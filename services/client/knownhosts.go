package client

import (
	"fmt"
	"net"
	"os"
	"path/filepath"

	gossh "golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/knownhosts"
)

func KnownHostsPath() (string, error) {
	dir, err := configDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "known_hosts"), nil
}

// HostKeyCallback returns a host key callback that checks against the known_hosts
// file, creating the file if absent. On first connect the host key is accepted
// and written (TOFU). The addr parameter is only used for normalisation — the
// actual key is checked per-address at connection time.
func HostKeyCallback(addr string) (gossh.HostKeyCallback, error) {
	path, err := KnownHostsPath()
	if err != nil {
		return nil, err
	}

	// Ensure the file exists so knownhosts.New doesn't error.
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}
	if _, err := os.Stat(path); os.IsNotExist(err) {
		f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0600)
		if err != nil {
			return nil, err
		}
		f.Close()
	}

	cb, err := knownhosts.New(path)
	if err != nil {
		return nil, err
	}

	return func(hostname string, remote net.Addr, key gossh.PublicKey) error {
		err := cb(hostname, remote, key)
		if err == nil {
			return nil
		}

		var kerr *knownhosts.KeyError
		if ke, ok := err.(*knownhosts.KeyError); ok {
			kerr = ke
		}

		if kerr != nil && len(kerr.Want) > 0 {
			// Key mismatch — hard fail to prevent MITM.
			return fmt.Errorf("host key mismatch for %s — run: ssh-keygen -R %s", hostname, hostname)
		}

		// Key not yet known → TOFU: write and accept.
		line := knownhosts.Line([]string{knownhosts.Normalize(hostname)}, key)
		f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0600)
		if err != nil {
			return fmt.Errorf("write known_hosts: %w", err)
		}
		defer f.Close()
		if _, err := fmt.Fprintln(f, line); err != nil {
			return fmt.Errorf("write known_hosts: %w", err)
		}
		return nil
	}, nil
}
