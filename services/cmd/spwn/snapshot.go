package main

import (
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/charmbracelet/huh"
	"github.com/charmbracelet/lipgloss"
	ltable "github.com/charmbracelet/lipgloss/table"
	"github.com/spf13/cobra"
	"github.com/spwn/spwn/services/client"
)

func snapshotCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "snapshot",
		Short: "manage VM snapshots",
	}
	cmd.AddCommand(
		snapshotListCmd(),
		snapshotTakeCmd(),
		snapshotRestoreCmd(),
		snapshotDeleteCmd(),
	)
	return cmd
}

func snapshotListCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "list <vm>",
		Short: "list snapshots for a VM",
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
			snaps, err := c.ListSnapshots(vm.ID)
			if err != nil {
				return err
			}

			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(snaps)
			}

			if len(snaps) == 0 {
				printHint(fmt.Sprintf("no snapshots for %s — take one with 'spwn snapshot take %s'", vm.Name, vm.Name))
				return nil
			}

			t := newTable(func(row, col int) lipgloss.Style {
				if row == ltable.HeaderRow {
					return styleHeader
				}
				if col == 2 || col == 3 {
					return styleDim
				}
				return styleVal
			})

			t.Headers("ID", "LABEL", "SIZE", "CREATED")
			for _, s := range snaps {
				label := ""
				if s.Label != nil {
					label = *s.Label
				}
				t.Row(
					s.ID,
					label,
					fmt.Sprintf("%.1fMB", float64(s.SizeBytes)/1024/1024),
					time.Unix(s.CreatedAt, 0).Format("2006-01-02 15:04"),
				)
			}
			fmt.Println(t.Render())
			return nil
		},
	}
}

func snapshotTakeCmd() *cobra.Command {
	var label string

	cmd := &cobra.Command{
		Use:   "take <vm>",
		Short: "take a snapshot",
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
			var lp *string
			if label != "" {
				lp = &label
			}
			snap, err := c.TakeSnapshot(vm.ID, lp)
			if err != nil {
				return err
			}

			jsonFlag, _ := cmd.Root().PersistentFlags().GetBool("json")
			if jsonFlag {
				return json.NewEncoder(os.Stdout).Encode(snap)
			}
			printOK(fmt.Sprintf("snapshot taken  %s", styleDim.Render(snap.ID)))
			return nil
		},
	}
	cmd.Flags().StringVar(&label, "label", "", "snapshot label")
	return cmd
}

func snapshotRestoreCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "restore <vm> <snapshot>",
		Short: "restore a snapshot",
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
			if err := c.RestoreSnapshot(vm.ID, args[1]); err != nil {
				return err
			}
			printOK(fmt.Sprintf("restoring %s from %s",
				styleVal.Render(vm.Name),
				styleDim.Render(args[1]),
			))
			return nil
		},
	}
}

func snapshotDeleteCmd() *cobra.Command {
	var force bool

	cmd := &cobra.Command{
		Use:   "delete <vm> <snapshot>",
		Short: "delete a snapshot",
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

			if !force {
				var confirmed bool
				err := huh.NewConfirm().
					Title(fmt.Sprintf("Delete snapshot %s?", args[1])).
					Description(fmt.Sprintf("Snapshot will be permanently removed from %s.", vm.Name)).
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

			if err := c.DeleteSnapshot(vm.ID, args[1]); err != nil {
				return err
			}
			printOK(fmt.Sprintf("deleted snapshot %s", styleDim.Render(args[1])))
			return nil
		},
	}
	cmd.Flags().BoolVar(&force, "force", false, "skip confirmation")
	return cmd
}
