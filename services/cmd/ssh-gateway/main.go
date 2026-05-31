package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	cssh "github.com/charmbracelet/ssh"
	"github.com/charmbracelet/wish"
	bm "github.com/charmbracelet/wish/bubbletea"
	"github.com/charmbracelet/wish/logging"
	"github.com/muesli/termenv"
	gossh "golang.org/x/crypto/ssh"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"github.com/spwn/spwn/services/client"
	agentpb "github.com/spwn/spwn/services/proto/agent"
	"github.com/spwn/spwn/services/tui"
)

// ── config ────────────────────────────────────────────────────────────────────

type gatewayConfig struct {
	listenAddr      string
	hostKeyPath     string
	controlPlaneURL string
	gatewaySecret   string
}

func loadConfig() gatewayConfig {
	return gatewayConfig{
		listenAddr:      envOr("SSH_GATEWAY_LISTEN_ADDR", ":2222"),
		hostKeyPath:     envOr("SSH_GATEWAY_HOST_KEY_PATH", "/var/lib/spwn/ssh_gateway_host_key"),
		controlPlaneURL: envOr("CONTROL_PLANE_HTTP_URL", "http://localhost:3019"),
		gatewaySecret:   envOr("GATEWAY_SECRET", "insecure"),
	}
}

func envOr(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

// ── control plane HTTP helpers ────────────────────────────────────────────────

type authResponse struct {
	OK        bool   `json:"ok"`
	AccountID string `json:"account_id"`
	Username  string `json:"username,omitempty"`
	Error     string `json:"error,omitempty"`
}

func (cfg *gatewayConfig) callAuth(path string, body map[string]string) (*authResponse, error) {
	data, _ := json.Marshal(body)
	req, err := http.NewRequestWithContext(
		context.Background(), "POST",
		cfg.controlPlaneURL+path,
		bytes.NewReader(data),
	)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+cfg.gatewaySecret)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	var out authResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, err
	}
	return &out, nil
}

func (cfg *gatewayConfig) createSession(accountID string) (string, error) {
	data, _ := json.Marshal(map[string]string{"account_id": accountID})
	req, err := http.NewRequestWithContext(
		context.Background(), "POST",
		cfg.controlPlaneURL+"/internal/gateway/session",
		bytes.NewReader(data),
	)
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+cfg.gatewaySecret)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	var out struct {
		Token string `json:"token"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return "", err
	}
	return out.Token, nil
}

type vmLookupResponse struct {
	VMID          string `json:"vm_id"`
	HostAgentAddr string `json:"host_agent_addr"`
	Status        string `json:"status"`
}

func (cfg *gatewayConfig) lookupVM(vmID string) (*vmLookupResponse, error) {
	url := fmt.Sprintf("%s/internal/gateway/vm?vm_id=%s", cfg.controlPlaneURL, vmID)
	req, err := http.NewRequestWithContext(context.Background(), "GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+cfg.gatewaySecret)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("vm lookup failed: %s", strings.TrimSpace(string(body)))
	}
	var out vmLookupResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ── gRPC console relay ────────────────────────────────────────────────────────

func relayConsole(ctx context.Context, agentAddr, vmID string, stdin io.Reader, stdout io.Writer, cols, rows uint32, term string, resizeCh <-chan [2]uint32) error {
	agentAddr = strings.TrimPrefix(agentAddr, "https://")
	agentAddr = strings.TrimPrefix(agentAddr, "http://")
	conn, err := grpc.NewClient(agentAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return fmt.Errorf("grpc dial %s: %w", agentAddr, err)
	}
	defer conn.Close()

	stream, err := agentpb.NewHostAgentClient(conn).StreamConsole(ctx)
	if err != nil {
		return fmt.Errorf("open stream: %w", err)
	}

	if err := stream.Send(&agentpb.ConsoleInput{
		VmId: vmID,
		Cols: cols,
		Rows: rows,
		Term: term,
	}); err != nil {
		return fmt.Errorf("send init frame: %w", err)
	}

	outputDone := make(chan error, 1)
	inputDone := make(chan error, 1)

	go func() {
		for {
			msg, err := stream.Recv()
			if err != nil {
				if err == io.EOF {
					outputDone <- nil
				} else {
					outputDone <- err
				}
				return
			}
			if _, err := stdout.Write(msg.Data); err != nil {
				outputDone <- err
				return
			}
		}
	}()

	go func() {
		buf := make([]byte, 4096)
		for {
			n, err := stdin.Read(buf)
			if n > 0 {
				if serr := stream.Send(&agentpb.ConsoleInput{Data: buf[:n]}); serr != nil {
					inputDone <- serr
					return
				}
			}
			if err != nil {
				_ = stream.CloseSend()
				inputDone <- nil
				return
			}
		}
	}()

	// Forward terminal resize events.
	if resizeCh != nil {
		go func() {
			for sz := range resizeCh {
				if serr := stream.Send(&agentpb.ConsoleInput{Cols: sz[0], Rows: sz[1]}); serr != nil {
					return
				}
			}
		}()
	}

	select {
	case err := <-outputDone:
		return err
	case err := <-inputDone:
		return err
	}
}

// ── auth ──────────────────────────────────────────────────────────────────────

type contextKey string

const accountIDKey contextKey = "account_id"
const usernameKey contextKey = "username"

func passwordAuth(cfg *gatewayConfig) cssh.PasswordHandler {
	return func(ctx cssh.Context, password string) bool {
		resp, err := cfg.callAuth("/internal/gateway/auth/password", map[string]string{
			"username": ctx.User(),
			"password": password,
		})
		if err != nil || !resp.OK {
			return false
		}
		ctx.SetValue(accountIDKey, resp.AccountID)
		if resp.Username != "" {
			ctx.SetValue(usernameKey, resp.Username)
		}
		return true
	}
}

func pubkeyAuth(cfg *gatewayConfig) cssh.PublicKeyHandler {
	return func(ctx cssh.Context, key cssh.PublicKey) bool {
		fp := gossh.FingerprintSHA256(key)
		resp, err := cfg.callAuth("/internal/gateway/auth/pubkey", map[string]string{
			"fingerprint": fp,
		})
		if err != nil || !resp.OK {
			return false
		}
		ctx.SetValue(accountIDKey, resp.AccountID)
		if resp.Username != "" {
			ctx.SetValue(usernameKey, resp.Username)
		}
		return true
	}
}

// ── direct VM relay ──────────────────────────────────────────────────────────

// tryDirectRelay checks if the SSH username matches a VM subdomain and relays
// directly to it. Returns true if the session was handled (VM found and relayed).
// Returns false if the username matches the account name (dashboard) or no VM found.
func tryDirectRelay(cfg *gatewayConfig, sess cssh.Session) bool {
	username := sess.User()

	accountID, _ := sess.Context().Value(accountIDKey).(string)
	accountUsername, _ := sess.Context().Value(usernameKey).(string)

	// If SSH username matches the account username, show the dashboard.
	if accountUsername != "" && username == accountUsername {
		return false
	}

	// Try to look up a VM whose subdomain matches the SSH username.
	vmInfo, err := cfg.lookupVM(username)
	if err != nil {
		// No VM with that subdomain — fall through to TUI.
		return false
	}

	if accountID == "" {
		if pk := sess.PublicKey(); pk != nil {
			fp := gossh.FingerprintSHA256(pk)
			if resp, err := cfg.callAuth("/internal/gateway/auth/pubkey", map[string]string{
				"fingerprint": fp,
			}); err == nil && resp.OK {
				accountID = resp.AccountID
			}
		}
	}
	if accountID == "" {
		fmt.Fprintln(sess.Stderr(), "error: authentication state missing")
		_ = sess.Exit(1)
		return true
	}

	log.Printf("direct relay: user %s -> vm %s (subdomain %s)", accountID, vmInfo.VMID, username)

	// Read PTY dimensions from the SSH session.
	pty, winCh, hasPty := sess.Pty()
	cols, rows := uint32(80), uint32(24)
	term := "xterm-256color"
	if hasPty {
		cols, rows = uint32(pty.Window.Width), uint32(pty.Window.Height)
		if pty.Term != "" {
			term = pty.Term
		}
	}

	// Convert the window change channel to our resize format.
	resizeCh := make(chan [2]uint32, 1)
	go func() {
		for win := range winCh {
			resizeCh <- [2]uint32{uint32(win.Width), uint32(win.Height)}
		}
		close(resizeCh)
	}()

	if err := relayConsole(sess.Context(), vmInfo.HostAgentAddr, vmInfo.VMID, sess, sess, cols, rows, term, resizeCh); err != nil {
		log.Printf("direct relay error: %v", err)
		fmt.Fprintf(sess.Stderr(), "connection error: %v\r\n", err)
		_ = sess.Exit(1)
		return true
	}
	_ = sess.Exit(0)
	return true
}

// ── TUI handler ───────────────────────────────────────────────────────────────

func tuiHandler(cfg *gatewayConfig) bm.Handler {
	return func(s cssh.Session) (tea.Model, []tea.ProgramOption) {
		accountID, _ := s.Context().Value(accountIDKey).(string)

		// Re-resolve from pubkey if password auth didn't set it.
		if accountID == "" {
			if pk := s.PublicKey(); pk != nil {
				fp := gossh.FingerprintSHA256(pk)
				if resp, err := cfg.callAuth("/internal/gateway/auth/pubkey", map[string]string{
					"fingerprint": fp,
				}); err == nil && resp.OK {
					accountID = resp.AccountID
				}
			}
		}

		if accountID == "" {
			fmt.Fprintln(s.Stderr(), "error: authentication state missing")
			_ = s.Exit(1)
			return nil, nil
		}

		token, err := cfg.createSession(accountID)
		if err != nil {
			log.Printf("create session for %s: %v", accountID, err)
			fmt.Fprintln(s.Stderr(), "error: could not create session")
			_ = s.Exit(1)
			return nil, nil
		}

		c := client.New(cfg.controlPlaneURL, token)

		pty, winCh, hasPty := s.Pty()
		initCols, initRows := uint32(80), uint32(24)
		initTerm := "xterm-256color"
		if hasPty {
			initCols, initRows = uint32(pty.Window.Width), uint32(pty.Window.Height)
			if pty.Term != "" {
				initTerm = pty.Term
			}
		}

		// Fan-out: relay needs its own resize channel so it doesn't steal
		// events from the TUI.
		relayWinCh := make(chan cssh.Window, 1)
		go func() {
			for win := range winCh {
				relayWinCh <- win
			}
			close(relayWinCh)
		}()

		connectFn := func(vmID string, stdin io.Reader, stdout io.Writer) error {
			vmInfo, err := cfg.lookupVM(vmID)
			if err != nil {
				return fmt.Errorf("vm lookup: %w", err)
			}
			resizeCh := make(chan [2]uint32, 1)
			go func() {
				for win := range relayWinCh {
					resizeCh <- [2]uint32{uint32(win.Width), uint32(win.Height)}
				}
				close(resizeCh)
			}()
			return relayConsole(s.Context(), vmInfo.HostAgentAddr, vmID, stdin, stdout, initCols, initRows, initTerm, resizeCh)
		}

		w, h := 80, 24
		if hasPty {
			w, h = pty.Window.Width, pty.Window.Height
		}

		return tui.NewSSHApp(c, w, h, connectFn), []tea.ProgramOption{tea.WithAltScreen()}
	}
}

// ── main ──────────────────────────────────────────────────────────────────────

func main() {
	cfg := loadConfig()

	if cfg.gatewaySecret == "" {
		log.Fatal("GATEWAY_SECRET must be set")
	}

	srv, err := wish.NewServer(
		wish.WithAddress(cfg.listenAddr),
		wish.WithHostKeyPath(cfg.hostKeyPath),
		wish.WithPasswordAuth(passwordAuth(&cfg)),
		wish.WithPublicKeyAuth(pubkeyAuth(&cfg)),
		wish.WithMiddleware(
			func(next cssh.Handler) cssh.Handler {
				return func(sess cssh.Session) {
					if !tryDirectRelay(&cfg, sess) {
						next(sess)
					}
				}
			},
			bm.MiddlewareWithColorProfile(tuiHandler(&cfg), termenv.TrueColor),
			logging.Middleware(),
		),
	)
	if err != nil {
		log.Fatalf("create server: %v", err)
	}

	l, err := net.Listen("tcp", cfg.listenAddr)
	if err != nil {
		log.Fatalf("listen %s: %v", cfg.listenAddr, err)
	}

	log.Printf("ssh-gateway listening on %s", cfg.listenAddr)

	done := make(chan struct{})
	go func() {
		defer close(done)
		if err := srv.Serve(l); err != nil {
			log.Printf("ssh-gateway: %v", err)
		}
	}()

	<-done

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
}
