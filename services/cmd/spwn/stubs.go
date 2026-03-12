package main

import (
	"encoding/json"
	"fmt"
	"io"
	"math/rand"
	"net"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/charmbracelet/lipgloss"
	ltable "github.com/charmbracelet/lipgloss/table"
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

// ── shared SSH dial ───────────────────────────────────────────────────────────

func dialSSHForVM(nameOrID string) (vm client.VM, conn *gossh.Client, err error) {
	cfg, err := loadConfigOrDefault()
	if err != nil {
		return vm, nil, err
	}
	creds, err := client.LoadCredentials()
	if err != nil {
		return vm, nil, err
	}
	c, err := client.NewAuthedClient()
	if err != nil {
		return vm, nil, err
	}

	vm, err = resolveVM(c, nameOrID)
	if err != nil {
		return vm, nil, err
	}
	if vm.Status != "running" {
		return vm, nil, fmt.Errorf("vm %q is %s (must be running)", vm.Name, vm.Status)
	}

	addr := cfg.GatewayAddrOrDefault()
	hkcb, err := client.HostKeyCallback(addr)
	if err != nil {
		return vm, nil, fmt.Errorf("host key: %w", err)
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

	conn, err = gossh.Dial("tcp", addr, sshCfg)
	if err != nil {
		return vm, nil, fmt.Errorf("ssh dial: %w", err)
	}
	return vm, conn, nil
}

// ── ssh ───────────────────────────────────────────────────────────────────────

func sshCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "ssh <vm>",
		Short: "open a shell in a VM",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			_, conn, err := dialSSHForVM(args[0])
			if err != nil {
				return err
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

// ── exec ──────────────────────────────────────────────────────────────────────

func execCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "exec <vm> -- <command> [args...]",
		Short: "run a command in a VM and stream its output",
		Long: `Run a one-shot command inside a VM over SSH.
The command's stdout and stderr are streamed to your terminal.
The exit code of spwn exec mirrors the exit code of the remote command.

Example:
  spwn exec myvm -- git pull
  spwn exec myvm -- sh -c 'echo hello && date'`,
		Args: cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			vmName := args[0]

			_, conn, err := dialSSHForVM(vmName)
			if err != nil {
				return err
			}
			defer conn.Close()

			sess, err := conn.NewSession()
			if err != nil {
				return fmt.Errorf("new session: %w", err)
			}
			defer sess.Close()

			remoteCmd := strings.Join(args[1:], " ")

			sess.Stdout = os.Stdout
			sess.Stderr = os.Stderr

			if err := sess.Run(remoteCmd); err != nil {
				if exitErr, ok := err.(*gossh.ExitError); ok {
					os.Exit(exitErr.ExitStatus())
				}
				return fmt.Errorf("exec: %w", err)
			}
			return nil
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

			vm, conn, err := dialSSHForVM(args[0])
			if err != nil {
				return err
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
				printHint("no keys registered — add one with 'spwn keys add <path>'")
				return nil
			}
			t := newTable(func(row, col int) lipgloss.Style {
				if row == ltable.HeaderRow {
					return styleHeader
				}
				if col == 1 {
					return styleDim
				}
				return styleVal
			})
			t.Headers("NAME", "FINGERPRINT")
			for _, k := range keys {
				t.Row(k.Name, k.Fingerprint)
			}
			fmt.Println(t.Render())
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

// ── lore ──────────────────────────────────────────────────────────────────────

var loreEntries = []string{
	"spwn was originally going to be called 'microbox'. the domain was taken.",
	"each firecracker VM boots in under 200ms. your laptop takes longer to wake from sleep.",
	"the name 'spwn' is what happens when you remove all the vowels from 'spawn' and pretend it's a brand.",
	"firecracker was built by AWS for Lambda and Fargate. you're running the same hypervisor as half the internet.",
	"a microVM is just a regular VM that went to the gym and stopped eating carbs.",
	"the TAP devices are named fc-tap-{n}. the 'fc' stands for firecracker, not the other thing.",
	"caddy rewrites your HTTP headers so fast it doesn't even bother telling you about it.",
	"the overlay filesystem means your VM's writes are stored separately from the base image. it's copy-on-write all the way down.",
	"spwn uses cgroup v2. cgroup v1 is still out there, haunting sysadmins.",
	"the SSH gateway is written in Go. the control plane is in Rust. they communicate in gRPC, which is written in feelings.",
	"your VM's IP is set via kernel boot args. no DHCP, no drama.",
	"squashfs is a compressed read-only filesystem. your rootfs is one of those. it's smaller than it looks.",
	"the jailer drops privileges before exec'ing firecracker. paranoia is a feature.",
	"if you've ever wondered what 172.16.0.0/16 is for — now you know.",
	"spwn keeps your session token in a file. treat it like a password. it basically is one.",
	"reconciliation runs on startup and fixes VMs stuck in 'starting' or 'stopping'. computers lie; spwn corrects them.",
	"every snapshot is just a memory state + disk diff. time travel, but for servers.",
	"caddy's admin API is bound to 127.0.0.1:2019. VMs cannot reach it. this is intentional.",
	"the control plane is written in rust. segfaults are impossible, but logic errors are still creative.",
	"firecracker uses less than 5mb of idle memory. your electron app just opened a new window and wept.",
	"everything is virtio. even the random number generator is a paravirtualized device. it's like asking the host for a dice roll.",
	"spwn leans heavily on kvm. without hardware virtualization extensions, this is just a very slow shell script.",
	"the platform SSH key is generated lazily on first console access. the VM doesn't know who it is until someone asks.",
	"restoring from a snapshot means loading a full memory image from disk and resuming execution. it's not time travel, it's just very aggressive bookmarking.",
	"caddy obtains certificates via let's encrypt automatically. you don't have to click 'i agree'. By using this Service you agree to all Terms outlined in this Document...",
	"overlayfs is the glue holding your writable files together. it's transparent, until it isn't.",
	"the kernel command line is long and ugly. it's the configuration file that didn't want to be a file.",
	"the gateway uses a connection pool. sharing is caring, especially for tcp sockets.",
	"the jailer uses seccomp filters. the vm can make very specific system calls. everything else is a one-way ticket to sigkill.",
	"you are root inside the vm. use it wisely. with great power comes the ability to `rm -rf /` instantly.",
	"context switching inside a microvm is cheap. cheaper than a context switch between two heavy processes on your host.",
	"logs are formatted as json. machines love it. humans squint at it.",
	"some communication is defined in protobuf. it's a contract between the gateway and the controller. lawyers aren't involved.",
	"cpu shares are handled via cgroups. your vm thinks it has 4 cores. it's actually fighting for crumbs.",
	"firecracker is a micro-hypervisor. it doesn't know how to be a bios, it just knows how to run a kernel.",
	"rebooting a VM doesn't wipe it. all the data is still there, and it's only gone when you delete it.",
	"the vm gets entropy from the host. it's borrowing chaos to generate order.",
	"if you see 'connection refused', check if the vm is actually running. introspection is a skill.",
	"rust compiles slowly because it's checking your work while you wait. it's the strict teacher you always needed.",
	"go code is 50% logic and 50% `if err != nil`. it's verbose, but at least you know exactly when it fails.",
	"rust uses a borrow checker. if you try to use memory after it's freed, the compiler laughs at you.",
	"go has a garbage collector. it pauses the world occasionally to take out the trash. rust takes out the trash as it goes.",
	"the control plane is rust because we prefer memory safety over developer happiness during compilation.",
	"the gateway is go only because Charm exists.",
	"go interfaces are satisfied implicitly. you don't say you implement it, you just do it. it's the honor system.",
	"rust's `Option` type replaces nulls. you have to unwrap the present before you can play with it.",
	"goroutines are cheap. spawning one thousand of them is just a tuesday for the go runtime.",
	"rust doesn't have a runtime. it brings nothing to the party but itself and the kernel.",
	"rust ownership rules are like a very possessive relationship. you can't look at that variable, i own it now.",
	"a rust panic unwinds the stack. a go panic crashes the goroutine. both are equally dramatic in the logs.",
	"the gRPC bridge is the handshake between the calm, collected rust core and the frantic, busy go gateway.",
	"rust generics are powerful. go generics exist now too, but we're still pretending it's 2015.",
	"the rust binary is stripped of symbols. it's small, hard to read, and efficient.",
}

func vmHistoryCmd() *cobra.Command {
	var limit int

	cmd := &cobra.Command{
		Use:   "history <name|id>",
		Short: "show VM event history",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			vm, err := resolveVM(c, args[0])
			if err != nil {
				return err
			}

			events, err := c.ListVMEvents(vm.ID, limit, nil)
			if err != nil {
				return fmt.Errorf("list events: %w", err)
			}

			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(events)
			}

			if len(events) == 0 {
				printHint(fmt.Sprintf("no events recorded for %s", vm.Name))
				return nil
			}

			fmt.Println(styleHeader.Render("  " + vm.Name + " — history"))
			fmt.Println()
			for _, e := range events {
				t := time.Unix(e.CreatedAt, 0)
				abs := styleDim.Render(t.Format("2006-01-02 15:04:05"))
				rel := styleDim.Render(relativeTime(t))
				fmt.Printf("  %s  %s  %s\n", abs, rel, styleVal.Render(e.Event))
			}
			return nil
		},
	}
	cmd.Flags().IntVar(&limit, "limit", 50, "maximum number of events to show")
	return cmd
}

func relativeTime(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return fmt.Sprintf("%ds ago", int(d.Seconds()))
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d.Minutes()))
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	default:
		return fmt.Sprintf("%dd ago", int(d.Hours()/24))
	}
}

func loreCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "lore",
		Short: "a random piece of spwn lore",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			src := rand.NewSource(time.Now().UnixNano())
			entry := loreEntries[rand.New(src).Intn(len(loreEntries))]
			fmt.Println(styleDim.Render("▸") + "  " + styleVal.Render(entry))
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
