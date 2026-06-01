package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/charmbracelet/huh"
	"github.com/charmbracelet/lipgloss"
	ltable "github.com/charmbracelet/lipgloss/table"
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
		vmCloneCmd(),
		vmHistoryCmd(),
		vmShareCmd(),
	)
	return cmd
}

func resolveVM(c *client.Client, nameOrID string) (client.VM, error) {
	if looksLikeID(nameOrID) {
		return c.GetVM(nameOrID)
	}

	if strings.Contains(nameOrID, ".") {
		vms, err := c.GetVMBySubdomain(nameOrID)
		if err == nil && len(vms) > 0 {
			return vms[0], nil
		}
		return client.VM{}, fmt.Errorf("no VM with subdomain %q", nameOrID)
	}

	// try name first ("epic vm" or "epic-vm")
	vms, err := c.GetVMByName(nameOrID)
	if err == nil && len(vms) > 0 {
		return vms[0], nil
	}

	vms, err = c.GetVMBySubdomain(nameOrID)
	if err == nil && len(vms) > 0 {
		return vms[0], nil
	}

	return client.VM{}, fmt.Errorf("no VM named or with subdomain %q", nameOrID)
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

			if len(vms) == 0 {
				printHint("no VMs yet — create one with 'spwn vm create <name>'")
				return nil
			}

			t := newTable(func(row, col int) lipgloss.Style {
				if row == ltable.HeaderRow {
					return styleHeader
				}
				vm := vms[row]
				switch col {
				case 0:
					return statusStyle(vm.Status)
				case 2:
					return statusStyle(vm.Status)
				case 3, 4:
					return styleDim
				}
				return styleVal
			})

			t.Headers("", "NAME", "STATUS", "SUBDOMAIN", "VCPUS / MEM")
			for _, vm := range vms {
				t.Row(
					statusSymbol(vm.Status),
					vm.Name,
					vm.Status,
					vm.Subdomain,
					fmt.Sprintf("%d / %dMB", vm.Vcpus, vm.MemoryMb),
				)
			}
			fmt.Println(t.Render())
			return nil
		},
	}
}

func vmCreateCmd() *cobra.Command {
	var vcpus int64
	var memMb int
	var image string
	var port int

	cmd := &cobra.Command{
		Use:   "create [name]",
		Short: "create a VM",
		Args:  cobra.RangeArgs(0, 1),
		RunE: func(cmd *cobra.Command, args []string) error {
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}

			name := ""
			if len(args) == 1 {
				name = args[0]
			}

			if name == "" {
				form := huh.NewForm(
					huh.NewGroup(
						huh.NewInput().
							Title("VM name").
							Value(&name),
						huh.NewInput().
							Title("Image").
							Value(&image),
					),
				)
				if err := form.Run(); err != nil {
					return err
				}
			}

			if name == "" {
				return fmt.Errorf("name is required")
			}

			vm, err := c.CreateVM(client.CreateVMRequest{
				Name:        name,
				Image:       image,
				Vcpus:       vcpus,
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
			printOK(fmt.Sprintf("created %s", styleVal.Render(vm.Name)))
			printHint(fmt.Sprintf("start it with: spwn vm start %s", vm.Name))
			return nil
		},
	}
	cmd.Flags().Int64Var(&vcpus, "vcpus", 1000, "cpu in millicores (1000 = 1 vCPU, 500 = 0.5 vCPU)")
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
			printOK(fmt.Sprintf("starting %s", styleVal.Render(vm.Name)))
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
			printOK(fmt.Sprintf("stopping %s", styleVal.Render(vm.Name)))
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
				var confirmed bool
				err := huh.NewConfirm().
					Title(fmt.Sprintf("Delete %s?", vm.Name)).
					Description("This is permanent. The VM and all its data will be gone.").
					Affirmative("delete").
					Negative("cancel").
					Value(&confirmed).
					Run()
				if err != nil {
					return err
				}
				if !confirmed {
					fmt.Println("aborted")
					return nil
				}
			}

			if err := c.DeleteVM(vm.ID); err != nil {
				return err
			}
			printOK(fmt.Sprintf("deleted %s", styleVal.Render(vm.Name)))
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

			// Build panel content.
			rows := []string{
				statusDot(vm.Status) + "  " + statusBadge(vm.Status),
				"",
				kvLine("subdomain", vm.Subdomain),
				kvLine("vcpus", fmt.Sprintf("%dm", vm.Vcpus)),
				kvLine("memory", fmt.Sprintf("%dMB", vm.MemoryMb)),
				kvLine("ip", vm.IPAddress),
			}
			if vm.ExposedPort > 0 {
				rows = append(rows, kvLine("port", fmt.Sprintf("%d", vm.ExposedPort)))
			}

			fmt.Println(panel(vm.Name, strings.Join(rows, "\n")))

			if len(events) > 0 {
				fmt.Println(styleHeader.Render("  events"))
				for _, e := range events {
					ts := styleDim.Render(time.Unix(e.CreatedAt, 0).Format("2006-01-02 15:04"))
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
			printOK(fmt.Sprintf("renamed to %s  %s",
				styleVal.Render(updated.Name),
				styleDim.Render(updated.Subdomain),
			))
			return nil
		},
	}
}

func vmCloneCmd() *cobra.Command {
	var includeMemory bool

	cmd := &cobra.Command{
		Use:   "clone <name|id> <new-name>",
		Short: "clone a VM",
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
			clone, err := c.CloneVM(vm.ID, client.CloneVMRequest{
				Name:          args[1],
				IncludeMemory: includeMemory,
			})
			if err != nil {
				return err
			}

			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(clone)
			}
			printOK(fmt.Sprintf("cloned %s → %s",
				styleVal.Render(vm.Name),
				styleVal.Render(clone.Name),
			))
			printHint(fmt.Sprintf("start it with: spwn vm start %s", clone.Name))
			return nil
		},
	}
	cmd.Flags().BoolVar(&includeMemory, "with-memory", false, "include memory state (source must be running; clone starts paused)")
	return cmd
}

func vmShareCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "share <vm>",
		Short: "manage VM access control",
	}
	var public bool
	var generate bool
	var revoke bool
	cmd.Flags().BoolVar(&public, "public", false, "make VM public")
	cmd.Flags().BoolVar(&public, "private", false, "make VM private")
	cmd.Flags().BoolVar(&generate, "generate", false, "generate a share link")
	cmd.Flags().BoolVar(&revoke, "revoke", false, "revoke share link")
	cmd.RunE = func(cmd *cobra.Command, args []string) error {
		if len(args) < 1 {
			return fmt.Errorf("usage: spwn vm share <vm>")
		}
		c, err := client.NewAuthedClient()
		if err != nil {
			return err
		}
		vm, err := resolveVM(c, args[0])
		if err != nil {
			return err
		}
		jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")

		if cmd.Flags().Changed("public") {
			updated, err := c.SetVmPublic(vm.ID, true)
			if err != nil {
				return err
			}
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(updated)
			}
			printOK(fmt.Sprintf("%s is now public", styleVal.Render(updated.Name)))
			printHint(fmt.Sprintf("access at: https://%s", updated.Subdomain+".spwn.town"))
			return nil
		}
		if cmd.Flags().Changed("private") {
			updated, err := c.SetVmPublic(vm.ID, false)
			if err != nil {
				return err
			}
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(updated)
			}
			printOK(fmt.Sprintf("%s is now private", styleVal.Render(updated.Name)))
			return nil
		}
		if generate {
			resp, err := c.GenerateShareToken(vm.ID)
			if err != nil {
				return err
			}
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(resp)
			}
			printOK("share link generated")
			printHint(fmt.Sprintf("https://%s/?token=%s", vm.Subdomain+".spwn.town", resp.Token))
			return nil
		}
		if revoke {
			err := c.RevokeShareToken(vm.ID)
			if err != nil {
				return err
			}
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(map[string]string{"status": "revoked"})
			}
			printOK("share link revoked")
			return nil
		}

		// Default: show current access status
		if jsonFlag {
			return json.NewEncoder(os.Stdout).Encode(map[string]any{
				"is_public":   vm.IsPublic,
				"share_token": vm.ShareToken,
			})
		}
		if vm.IsPublic {
			printOK(fmt.Sprintf("%s is public", styleVal.Render(vm.Name)))
		} else {
			printOK(fmt.Sprintf("%s is private", styleVal.Render(vm.Name)))
		}
		if vm.ShareToken != "" {
			printHint(fmt.Sprintf("share link: https://%s/?token=%s", vm.Subdomain+".spwn.town", vm.ShareToken))
		}
		return nil
	}
	return cmd
}
