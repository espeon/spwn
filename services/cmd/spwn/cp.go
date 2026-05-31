package main

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/pkg/sftp"
	"github.com/spf13/cobra"
)

// vmPath splits "vmname:/some/path" into ("vmname", "/some/path").
// Returns ("", s) if s has no colon prefix that looks like a VM ref.
func parseVMPath(s string) (vm, path string, ok bool) {
	idx := strings.Index(s, ":")
	if idx <= 0 {
		return "", s, false
	}
	return s[:idx], s[idx+1:], true
}

func cpCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "cp <source> <destination>",
		Short: "copy files to or from a VM",
		Long: `Copy files between the local filesystem and a VM.

  spwn cp ./file.txt myvm:/home/user/        upload a file
  spwn cp myvm:/var/log/app.log ./           download a file
  spwn cp myvm:/var/log/app.log -            stream file to stdout`,
		Args: cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			src, dst := args[0], args[1]

			srcVM, srcPath, srcIsVM := parseVMPath(src)
			dstVM, dstPath, dstIsVM := parseVMPath(dst)

			switch {
			case srcIsVM && dstIsVM:
				return fmt.Errorf("cannot copy between two VMs")
			case !srcIsVM && !dstIsVM:
				return fmt.Errorf("at least one of source or destination must be a VM path (vm:/path)")
			case srcIsVM:
				return cpDownload(srcVM, srcPath, dstPath)
			default:
				return cpUpload(src, dstVM, dstPath)
			}
		},
	}
}

func cpUpload(localSrc, vmName, remoteDst string) error {
	_, conn, err := dialSSHForVM(vmName)
	if err != nil {
		return err
	}
	defer conn.Close()

	sc, err := sftp.NewClient(conn)
	if err != nil {
		return fmt.Errorf("sftp: %w", err)
	}
	defer sc.Close()

	f, err := os.Open(localSrc)
	if err != nil {
		return fmt.Errorf("open %s: %w", localSrc, err)
	}
	defer f.Close()

	// If dst is a directory, append the source filename.
	dst := remoteDst
	if info, err := sc.Stat(remoteDst); err == nil && info.IsDir() {
		dst = filepath.Join(remoteDst, filepath.Base(localSrc))
	}

	rf, err := sc.Create(dst)
	if err != nil {
		return fmt.Errorf("create remote %s: %w", dst, err)
	}
	defer rf.Close()

	n, err := io.Copy(rf, f)
	if err != nil {
		return fmt.Errorf("copy: %w", err)
	}
	fmt.Fprintf(os.Stderr, "%s → %s:%s (%s)\n", localSrc, vmName, dst, humanBytes(n))
	return nil
}

func cpDownload(vmName, remoteSrc, localDst string) error {
	_, conn, err := dialSSHForVM(vmName)
	if err != nil {
		return err
	}
	defer conn.Close()

	sc, err := sftp.NewClient(conn)
	if err != nil {
		return fmt.Errorf("sftp: %w", err)
	}
	defer sc.Close()

	rf, err := sc.Open(remoteSrc)
	if err != nil {
		return fmt.Errorf("open remote %s: %w", remoteSrc, err)
	}
	defer rf.Close()

	// "-" means stdout.
	if localDst == "-" {
		_, err = io.Copy(os.Stdout, rf)
		return err
	}

	// If dst is an existing directory, write filename inside it.
	dst := localDst
	if info, err := os.Stat(localDst); err == nil && info.IsDir() {
		dst = filepath.Join(localDst, filepath.Base(remoteSrc))
	}

	lf, err := os.Create(dst)
	if err != nil {
		return fmt.Errorf("create %s: %w", dst, err)
	}
	defer lf.Close()

	n, err := io.Copy(lf, rf)
	if err != nil {
		return fmt.Errorf("copy: %w", err)
	}
	fmt.Fprintf(os.Stderr, "%s:%s → %s (%s)\n", vmName, remoteSrc, dst, humanBytes(n))
	return nil
}

func humanBytes(n int64) string {
	const unit = 1024
	if n < unit {
		return fmt.Sprintf("%d B", n)
	}
	div, exp := int64(unit), 0
	for v := n / unit; v >= unit; v /= unit {
		div *= unit
		exp++
	}
	return fmt.Sprintf("%.1f %cB", float64(n)/float64(div), "KMGTPE"[exp])
}
