package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/spf13/cobra"
	"github.com/spwn/spwn/services/client"
)

func vmCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "vm",
		Short: "manage VMs",
	}
	cmd.AddCommand(
		vmListCmd(),
		vmCreateCmd(),
		vmStartCmd(),
		vmStopCmd(),
		vmDeleteCmd(),
		vmStatusCmd(),
		vmRenameCmd(),
	)
	return cmd
}

func resolveVM(c *client.Client, nameOrID string) (client.VM, error) {
	// Try by name first (short-circuit if it looks like a UUID).
	if !looksLikeID(nameOrID) {
		vms, err := c.GetVMByName(nameOrID)
		if err == nil && len(vms) > 0 {
			return vms[0], nil
		}
	}
	return c.GetVM(nameOrID)
}

func looksLikeID(s string) bool {
	return len(s) == 36 && strings.Count(s, "-") == 4
}

func vmListCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "list",
		Short: "list VMs",
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			vms, err := c.ListVMs()
			if err != nil {
				return err
			}
			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(vms)
			}
			w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
			fmt.Fprintln(w, "NAME\tSTATUS\tSUBDOMAIN\tVCORES\tMEM")
			for _, vm := range vms {
				fmt.Fprintf(w, "%s\t%s\t%s\t%d\t%dMB\n",
					vm.Name, vm.Status, vm.Subdomain, vm.Vcores, vm.MemoryMb)
			}
			return w.Flush()
		},
	}
}

func vmCreateCmd() *cobra.Command {
	var vcores int
	var memMb int
	var image string
	var port int

	cmd := &cobra.Command{
		Use:   "create <name>",
		Short: "create a VM",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			vm, err := c.CreateVM(client.CreateVMRequest{
				Name:        args[0],
				Image:       image,
				Vcores:      vcores,
				MemoryMb:    memMb,
				ExposedPort: port,
			})
			if err != nil {
				return err
			}
			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(vm)
			}
			fmt.Printf("created %s (%s)\n", vm.Name, vm.ID)
			return nil
		},
	}
	cmd.Flags().IntVar(&vcores, "vcores", 2, "number of vCPUs")
	cmd.Flags().IntVar(&memMb, "memory", 512, "memory in MB")
	cmd.Flags().StringVar(&image, "image", "ubuntu", "root filesystem image")
	cmd.Flags().IntVar(&port, "port", 8080, "exposed port")
	return cmd
}

func vmStartCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "start <name|id>",
		Short: "start a stopped VM",
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
			if err := c.StartVM(vm.ID); err != nil {
				return err
			}
			fmt.Printf("starting %s\n", vm.Name)
			return nil
		},
	}
}

func vmStopCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "stop <name|id>",
		Short: "stop a running VM",
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
			if err := c.StopVM(vm.ID); err != nil {
				return err
			}
			fmt.Printf("stopping %s\n", vm.Name)
			return nil
		},
	}
}

func vmDeleteCmd() *cobra.Command {
	var force bool

	cmd := &cobra.Command{
		Use:   "delete <name|id>",
		Short: "delete a VM",
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
			if !force {
				fmt.Printf("delete %s? [y/N] ", vm.Name)
				var confirm string
				fmt.Scanln(&confirm)
				if confirm != "y" && confirm != "Y" {
					fmt.Println("aborted")
					return nil
				}
			}
			if err := c.DeleteVM(vm.ID); err != nil {
				return err
			}
			fmt.Printf("deleted %s\n", vm.Name)
			return nil
		},
	}
	cmd.Flags().BoolVar(&force, "force", false, "skip confirmation")
	return cmd
}

func vmStatusCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "status <name|id>",
		Short: "show VM status and recent events",
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
			events, _ := c.ListVMEvents(vm.ID, 10, nil)

			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(struct {
					VM     client.VM        `json:"vm"`
					Events []client.VMEvent `json:"events"`
				}{vm, events})
			}

			fmt.Printf("%s  %s\n", vm.Name, vm.Status)
			fmt.Printf("subdomain: %s   vcores: %d   mem: %dMB\n",
				vm.Subdomain, vm.Vcores, vm.MemoryMb)
			if len(events) > 0 {
				fmt.Println("\nrecent events:")
				for _, e := range events {
					ts := time.Unix(e.CreatedAt, 0).Format("2006-01-02 15:04")
					fmt.Printf("  %s  %s\n", ts, e.Event)
				}
			}
			return nil
		},
	}
}

func vmRenameCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "rename <name|id> <new-name>",
		Short: "rename a VM",
		Args:  cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}
			vm, err := resolveVM(c, args[0])
			if err != nil {
				return err
			}
			newName := args[1]
			updated, err := c.PatchVM(vm.ID, client.PatchVMRequest{Name: &newName})
			if err != nil {
				return err
			}
			fmt.Printf("renamed to %s (%s)\n", updated.Name, updated.Subdomain)
			return nil
		},
	}
}
