package main

import (
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/pkg/browser"
	"github.com/spf13/cobra"
	"github.com/spwn/spwn/services/client"
)

func loginCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "login",
		Short: "authenticate via browser",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := client.LoadConfig()
			if err != nil {
				return err
			}
			c := client.New(cfg.APIURL, "")

			init, err := c.CLIInit(cfg.APIURL)
			if err != nil {
				return fmt.Errorf("could not initiate login: %w", err)
			}

			fmt.Printf("opening browser to authorize...\n")
			fmt.Printf("url: %s\n\n", init.BrowserURL)

			if err := browser.OpenURL(init.BrowserURL); err != nil {
				fmt.Fprintf(os.Stderr, "could not open browser automatically — visit the URL above\n")
			}

			fmt.Print("waiting for authorization")
			deadline := time.Now().Add(time.Duration(init.ExpiresIn) * time.Second)

			for time.Now().Before(deadline) {
				time.Sleep(2 * time.Second)
				fmt.Print(".")

				poll, err := c.CLIPoll(init.Code)
				if err != nil {
					continue
				}

				switch poll.Status {
				case "authorized":
					fmt.Println()
					if err := client.SaveCredentials(client.Credentials{Token: *poll.Token}); err != nil {
						return fmt.Errorf("could not save credentials: %w", err)
					}
					if serverCfg, err := c.GetServerConfig(); err == nil && serverCfg.SSHGatewayAddr != "" {
						if localCfg, err := client.LoadConfig(); err == nil {
							localCfg.GatewayAddr = serverCfg.SSHGatewayAddr
							_ = client.SaveConfig(localCfg)
						}
					}
					fmt.Println("logged in successfully")
					return nil
				case "denied":
					fmt.Println()
					return fmt.Errorf("authorization denied")
				case "expired":
					fmt.Println()
					return fmt.Errorf("code expired — run 'spwn login' again")
				}
			}

			fmt.Println()
			return fmt.Errorf("timed out waiting for authorization")
		},
	}
}

func logoutCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logout",
		Short: "clear stored credentials",
		RunE: func(cmd *cobra.Command, args []string) error {
			if err := client.ClearCredentials(); err != nil {
				return err
			}
			fmt.Println("logged out")
			return nil
		},
	}
}

func whoamiCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "whoami",
		Short: "show current account and quota",
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			me, err := c.Me()
			if err != nil {
				return err
			}
			jsonFlag, _ := cmd.Flags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(me)
			}
			fmt.Printf("%s (%s)\n", me.Email, me.Username)
			fmt.Printf("vcores: %d   ram: %dMB   vms: %d\n",
				me.VcpuLimit, me.MemLimitMb, me.VmLimit)
			return nil
		},
	}
}
