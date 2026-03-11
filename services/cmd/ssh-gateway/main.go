package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	neturl "net/url"
	"os"
	"strings"
	"time"

	cssh "github.com/charmbracelet/ssh"
	"github.com/charmbracelet/wish"
	"github.com/charmbracelet/wish/activeterm"
	"github.com/charmbracelet/wish/logging"
	gossh "golang.org/x/crypto/ssh"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	agentpb "github.com/spwn/spwn/services/proto/agent"
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

type vmLookupResponse struct {
	VMID          string `json:"vm_id"`
	HostAgentAddr string `json:"host_agent_addr"`
	VMIP          string `json:"vm_ip"`
	Status        string `json:"status"`
	ExposedPort   int    `json:"exposed_port"`
}

func (cfg *gatewayConfig) callAuth(path string, body map[string]string) (*authResponse, error) {
	data, _ := json.Marshal(body)
	req, err := http.NewRequestWithContext(
		context.Background(), "POST",
		cfg.controlPlaneURL+path,
		strings.NewReader(string(data)),
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

func (cfg *gatewayConfig) lookupVM(username, accountUsername string) (*vmLookupResponse, error) {
	var query string
	if strings.Contains(username, ".") {
		query = "subdomain=" + neturl.QueryEscape(username)
	} else if accountUsername != "" {
		query = "subdomain=" + neturl.QueryEscape(username+"."+accountUsername)
	} else {
		query = "vm_id=" + neturl.QueryEscape(username)
	}
	url := fmt.Sprintf("%s/internal/gateway/vm?%s", cfg.controlPlaneURL, query)
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
		return nil, fmt.Errorf("vm lookup failed (status %d)", resp.StatusCode)
	}
	var out vmLookupResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ── gRPC console relay ────────────────────────────────────────────────────────

func relayConsole(ctx context.Context, agentAddr, vmID string, s cssh.Session) error {
	agentAddr = strings.TrimPrefix(agentAddr, "https://")
	agentAddr = strings.TrimPrefix(agentAddr, "http://")
	conn, err := grpc.NewClient(agentAddr,
		grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return fmt.Errorf("grpc dial %s: %w", agentAddr, err)
	}
	defer conn.Close()

	stream, err := agentpb.NewHostAgentClient(conn).StreamConsole(ctx)
	if err != nil {
		return fmt.Errorf("open stream: %w", err)
	}

	// First frame: identify the VM.
	if err := stream.Send(&agentpb.ConsoleInput{VmId: vmID}); err != nil {
		return fmt.Errorf("send vm_id: %w", err)
	}

	errCh := make(chan error, 2)

	// gRPC output → SSH session
	go func() {
		for {
			msg, err := stream.Recv()
			if err != nil {
				if err == io.EOF {
					errCh <- nil
				} else {
					errCh <- err
				}
				return
			}
			if _, err := s.Write(msg.Data); err != nil {
				errCh <- err
				return
			}
		}
	}()

	// SSH session → gRPC input
	go func() {
		buf := make([]byte, 4096)
		for {
			n, err := s.Read(buf)
			if n > 0 {
				if serr := stream.Send(&agentpb.ConsoleInput{Data: buf[:n]}); serr != nil {
					errCh <- serr
					return
				}
			}
			if err != nil {
				_ = stream.CloseSend()
				if err == io.EOF {
					errCh <- nil
				} else {
					errCh <- err
				}
				return
			}
		}
	}()

	return <-errCh
}

// ── session handler middleware ────────────────────────────────────────────────

type contextKey string

const accountIDKey contextKey = "account_id"
const usernameKey contextKey = "username"

func sessionMiddleware(cfg *gatewayConfig) wish.Middleware {
	return func(_ cssh.Handler) cssh.Handler {
		return func(s cssh.Session) {
			username := s.User()

			// Prefer pubkey auth: re-resolve the account ID from the key that
			// was actually used to authenticate this session. The pubkey handler
			// runs twice (probe + real) and context values set during the probe
			// are not visible here, so we look it up again.
			var accountID, accountUsername string
			if pk := s.PublicKey(); pk != nil {
				fp := gossh.FingerprintSHA256(pk)
				resp, err := cfg.callAuth("/internal/gateway/auth/pubkey", map[string]string{
					"fingerprint": fp,
				})
				if err == nil && resp.OK {
					accountID = resp.AccountID
					accountUsername = resp.Username
				}
			}

			// Fall back to the value set by password auth.
			if accountID == "" {
				accountID, _ = s.Context().Value(accountIDKey).(string)
				accountUsername, _ = s.Context().Value(usernameKey).(string)
			}

			if accountID == "" {
				fmt.Fprintln(s.Stderr(), "error: authentication state missing")
				_ = s.Exit(1)
				return
			}

			vm, err := cfg.lookupVM(username, accountUsername)
			if err != nil {
				fmt.Fprintf(s.Stderr(), "error: %v\r\n", err)
				_ = s.Exit(1)
				return
			}
			if vm.Status != "running" {
				fmt.Fprintf(s.Stderr(), "vm '%s' is %s (must be running)\r\n", username, vm.Status)
				_ = s.Exit(1)
				return
			}

			if err := relayConsole(s.Context(), vm.HostAgentAddr, vm.VMID, s); err != nil {
				log.Printf("relay ended: vm=%s err=%v", vm.VMID, err)
			}
		}
	}
}

// ── auth ──────────────────────────────────────────────────────────────────────

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
		ctx.SetValue(usernameKey, resp.Username)
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
		ctx.SetValue(usernameKey, resp.Username)
		return true
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
			sessionMiddleware(&cfg),
			logging.Middleware(),
			activeterm.Middleware(),
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
