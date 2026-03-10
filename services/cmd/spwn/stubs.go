package main

import (
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"strings"

	"github.com/spf13/cobra"
	gossh "golang.org/x/crypto/ssh"
	"golang.org/x/term"

	"github.com/spwn/spwn/services/client"
)

func loadDefaultSigners() []gossh.Signer {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil
	}
	candidates := []string{
		home + "/.ssh/id_rsa",
		home + "/.ssh/id_ed25519",
	}
	var signers []gossh.Signer
	for _, path := range candidates {
		data, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		signer, err := gossh.ParsePrivateKey(data)
		if err != nil {
			continue
		}
		signers = append(signers, signer)
	}
	return signers
}

// ── ssh ───────────────────────────────────────────────────────────────────────

func sshCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "ssh <vm>",
		Short: "open a shell in a VM",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadConfigOrDefault()
			if err != nil {
				return err
			}
			creds, err := client.LoadCredentials()
			if err != nil {
				return err
			}
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}

			vms, err := c.GetVMByName(args[0])
			if err != nil {
				return fmt.Errorf("look up vm: %w", err)
			}
			if len(vms) == 0 {
				return fmt.Errorf("no vm named %q", args[0])
			}
			vm := vms[0]
			if vm.Status != "running" {
				return fmt.Errorf("vm %q is %s (must be running)", vm.Name, vm.Status)
			}

			addr := cfg.GatewayAddrOrDefault()
			hkcb, err := client.HostKeyCallback(addr)
			if err != nil {
				return fmt.Errorf("host key: %w", err)
			}

			authMethods := []gossh.AuthMethod{}
			if signers := loadDefaultSigners(); len(signers) > 0 {
				authMethods = append(authMethods, gossh.PublicKeys(signers...))
			}
			authMethods = append(authMethods, gossh.Password(creds.Token))

			sshCfg := &gossh.ClientConfig{
				User:            vm.ID,
				Auth:            authMethods,
				HostKeyCallback: hkcb,
			}

			conn, err := gossh.Dial("tcp", addr, sshCfg)
			if err != nil {
				return fmt.Errorf("ssh dial: %w", err)
			}
			defer conn.Close()

			sess, err := conn.NewSession()
			if err != nil {
				return fmt.Errorf("new session: %w", err)
			}
			defer sess.Close()

			fd := int(os.Stdin.Fd())
			w, h, err := term.GetSize(fd)
			if err != nil {
				w, h = 80, 24
			}

			modes := gossh.TerminalModes{
				gossh.ECHO:          1,
				gossh.TTY_OP_ISPEED: 14400,
				gossh.TTY_OP_OSPEED: 14400,
			}
			if err := sess.RequestPty("xterm-256color", h, w, modes); err != nil {
				return fmt.Errorf("pty request: %w", err)
			}

			sess.Stdin = os.Stdin
			sess.Stdout = os.Stdout
			sess.Stderr = os.Stderr

			oldState, err := term.MakeRaw(fd)
			if err != nil {
				return fmt.Errorf("make raw: %w", err)
			}
			defer term.Restore(fd, oldState)

			if err := sess.Shell(); err != nil {
				return fmt.Errorf("start shell: %w", err)
			}
			return sess.Wait()
		},
	}
}

// ── tunnel ────────────────────────────────────────────────────────────────────

func tunnelCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "tunnel <vm> <local-port:remote-port>",
		Short: "forward a local port to a port inside a VM",
		Args:  cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadConfigOrDefault()
			if err != nil {
				return err
			}
			creds, err := client.LoadCredentials()
			if err != nil {
				return err
			}
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}

			vms, err := c.GetVMByName(args[0])
			if err != nil {
				return fmt.Errorf("look up vm: %w", err)
			}
			if len(vms) == 0 {
				return fmt.Errorf("no vm named %q", args[0])
			}
			vm := vms[0]
			if vm.Status != "running" {
				return fmt.Errorf("vm %q is %s (must be running)", vm.Name, vm.Status)
			}

			parts := strings.SplitN(args[1], ":", 2)
			if len(parts) != 2 {
				return fmt.Errorf("port spec must be <local>:<remote>")
			}
			localPort, remotePort := parts[0], parts[1]
			if _, err := strconv.Atoi(localPort); err != nil {
				return fmt.Errorf("invalid local port: %s", localPort)
			}
			if _, err := strconv.Atoi(remotePort); err != nil {
				return fmt.Errorf("invalid remote port: %s", remotePort)
			}

			addr := cfg.GatewayAddrOrDefault()
			hkcb, err := client.HostKeyCallback(addr)
			if err != nil {
				return fmt.Errorf("host key: %w", err)
			}

			authMethods := []gossh.AuthMethod{}
			if signers := loadDefaultSigners(); len(signers) > 0 {
				authMethods = append(authMethods, gossh.PublicKeys(signers...))
			}
			authMethods = append(authMethods, gossh.Password(creds.Token))

			sshCfg := &gossh.ClientConfig{
				User:            vm.ID,
				Auth:            authMethods,
				HostKeyCallback: hkcb,
			}

			conn, err := gossh.Dial("tcp", addr, sshCfg)
			if err != nil {
				return fmt.Errorf("ssh dial: %w", err)
			}
			defer conn.Close()

			ln, err := net.Listen("tcp", "127.0.0.1:"+localPort)
			if err != nil {
				return fmt.Errorf("listen on :%s: %w", localPort, err)
			}
			defer ln.Close()

			fmt.Fprintf(os.Stderr, "forwarding 127.0.0.1:%s → %s:%s\n", localPort, vm.Name, remotePort)

			for {
				local, err := ln.Accept()
				if err != nil {
					return nil
				}
				go func(local net.Conn) {
					defer local.Close()
					remote, err := conn.Dial("tcp", "127.0.0.1:"+remotePort)
					if err != nil {
						fmt.Fprintf(os.Stderr, "tunnel dial: %v\n", err)
						return
					}
					defer remote.Close()
					done := make(chan struct{}, 2)
					go func() { io.Copy(remote, local); done <- struct{}{} }()
					go func() { io.Copy(local, remote); done <- struct{}{} }()
					<-done
				}(local)
			}
		},
	}
}

// ── keys ──────────────────────────────────────────────────────────────────────

func keysCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "keys",
		Short: "manage SSH public keys",
	}
	cmd.AddCommand(keysListCmd(), keysAddCmd(), keysRemoveCmd())
	return cmd
}

func keysListCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "list",
		Short: "list registered SSH public keys",
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			keys, err := c.ListSSHKeys()
			if err != nil {
				return err
			}
			if len(keys) == 0 {
				fmt.Println("no keys registered — add one with 'spwn keys add'")
				return nil
			}
			for _, k := range keys {
				fmt.Printf("%-20s  %s\n", k.Name, k.Fingerprint)
			}
			return nil
		},
	}
}

func keysAddCmd() *cobra.Command {
	var name string
	cmd := &cobra.Command{
		Use:   "add <path>",
		Short: "register a public key",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			data, err := os.ReadFile(args[0])
			if err != nil {
				return fmt.Errorf("read key file: %w", err)
			}
			keyData := strings.TrimSpace(string(data))

			if name == "" {
				parts := strings.Fields(keyData)
				if len(parts) >= 3 {
					name = parts[2]
				} else {
					name = "key"
				}
			}

			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			key, err := c.AddSSHKey(name, keyData)
			if err != nil {
				return err
			}
			fmt.Printf("added %s (%s)\n", key.Name, key.Fingerprint)
			return nil
		},
	}
	cmd.Flags().StringVar(&name, "name", "", "key name (defaults to key comment)")
	return cmd
}

func keysRemoveCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "remove <id>",
		Short: "remove a registered key",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			if err := c.DeleteSSHKey(args[0]); err != nil {
				return err
			}
			fmt.Printf("removed key %s\n", args[0])
			return nil
		},
	}
}

// ── config ────────────────────────────────────────────────────────────────────

func configCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "config",
		Short: "manage CLI configuration",
	}

	getCmd := &cobra.Command{
		Use:   "get <key>",
		Short: "get a config value",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadConfigOrDefault()
			if err != nil {
				return err
			}
			switch args[0] {
			case "api-url":
				fmt.Println(cfg.APIURL)
			case "gateway-addr":
				fmt.Println(cfg.GatewayAddrOrDefault())
			case "default-vcores":
				fmt.Println(cfg.DefaultVcores)
			case "default-memory":
				fmt.Println(cfg.DefaultMemMb)
			default:
				return fmt.Errorf("unknown key %q (valid: api-url, gateway-addr, default-vcores, default-memory)", args[0])
			}
			return nil
		},
	}

	setCmd := &cobra.Command{
		Use:   "set <key> <value>",
		Short: "set a config value",
		Args:  cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadConfigOrDefault()
			if err != nil {
				return err
			}
			switch args[0] {
			case "api-url":
				cfg.APIURL = args[1]
			case "gateway-addr":
				cfg.GatewayAddr = args[1]
			default:
				return fmt.Errorf("unknown key %q (settable: api-url, gateway-addr)", args[0])
			}
			if err := saveConfig(cfg); err != nil {
				return err
			}
			fmt.Printf("set %s = %s\n", args[0], args[1])
			return nil
		},
	}

	cmd.AddCommand(getCmd, setCmd)
	return cmd
}
