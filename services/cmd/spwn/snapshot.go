package main

import (
	"encoding/json"
	"fmt"
	"os"
	"text/tabwriter"
	"time"

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
				fmt.Println("no snapshots")
				return nil
			}
			w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
			fmt.Fprintln(w, "ID\tLABEL\tSIZE\tCREATED")
			for _, s := range snaps {
				label := ""
				if s.Label != nil {
					label = *s.Label
				}
				created := time.Unix(s.CreatedAt, 0).Format("2006-01-02 15:04")
				size := fmt.Sprintf("%.1fMB", float64(s.SizeBytes)/1024/1024)
				fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", s.ID, label, size, created)
			}
			return w.Flush()
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
			fmt.Printf("snapshot taken: %s\n", snap.ID)
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
			fmt.Printf("restoring %s from %s\n", vm.Name, args[1])
			return nil
		},
	}
}

func snapshotDeleteCmd() *cobra.Command {
	return &cobra.Command{
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
			if err := c.DeleteSnapshot(vm.ID, args[1]); err != nil {
				return err
			}
			fmt.Printf("deleted snapshot %s\n", args[1])
			return nil
		},
	}
}
