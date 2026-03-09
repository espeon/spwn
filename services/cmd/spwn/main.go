package main

import (
	"fmt"
	"os"

	"golang.org/x/term"

	"github.com/spwn/spwn/services/client"
	"github.com/spwn/spwn/services/tui"
)

func main() {
	// No subcommand + stdout is a TTY → launch TUI.
	if len(os.Args) == 1 && term.IsTerminal(int(os.Stdout.Fd())) {
		c, err := client.NewAuthedClient()
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			fmt.Fprintln(os.Stderr, "run 'spwn login' first")
			os.Exit(1)
		}
		if err := tui.Run(c); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}

	if err := rootCmd().Execute(); err != nil {
		os.Exit(1)
	}
}
