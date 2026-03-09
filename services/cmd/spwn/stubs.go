package main

import (
	"fmt"

	"github.com/spf13/cobra"
)

func sshCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "ssh [vm]",
		Short: "open a shell in a VM (coming in phase 8)",
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println("SSH gateway not yet available.")
			fmt.Println("Coming in phase 8 — stay tuned.")
			return nil
		},
	}
}

func tunnelCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "tunnel <vm> <local:remote>",
		Short: "forward a local port to a VM port (coming in phase 8)",
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println("Port tunneling not yet available.")
			fmt.Println("Coming in phase 8 — stay tuned.")
			return nil
		},
	}
}

func keysCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "keys",
		Short: "manage SSH public keys (coming in phase 8)",
	}
	stub := func(use, short string) *cobra.Command {
		return &cobra.Command{
			Use:   use,
			Short: short,
			RunE: func(cmd *cobra.Command, args []string) error {
				fmt.Println("SSH key management not yet available.")
				fmt.Println("Coming in phase 8 — stay tuned.")
				return nil
			},
		}
	}
	cmd.AddCommand(
		stub("list", "list registered SSH public keys"),
		stub("add <name> <path>", "register a public key"),
		stub("remove <name|id>", "remove a key"),
	)
	return cmd
}

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
			case "default-vcores":
				fmt.Println(cfg.DefaultVcores)
			case "default-memory":
				fmt.Println(cfg.DefaultMemMb)
			default:
				return fmt.Errorf("unknown key: %s", args[0])
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
			default:
				return fmt.Errorf("unknown key: %s (valid: api-url)", args[0])
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
