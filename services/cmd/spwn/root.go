package main

import (
	"github.com/spf13/cobra"
)

func rootCmd() *cobra.Command {
	root := &cobra.Command{
		Use:   "spwn",
		Short: "spwn — firecracker VM platform CLI",
		Long:  "manage spwn VMs from the terminal. run without arguments to open the TUI.",
	}

	root.AddCommand(
		loginCmd(),
		logoutCmd(),
		whoamiCmd(),
		vmCmd(),
		snapshotCmd(),
		sshCmd(),
		tunnelCmd(),
		keysCmd(),
		configCmd(),
	)

	root.PersistentFlags().Bool("json", false, "output as JSON")
	root.PersistentFlags().Bool("quiet", false, "suppress non-essential output")

	return root
}
