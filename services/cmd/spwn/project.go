package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/charmbracelet/huh"
	"github.com/spf13/cobra"
	"github.com/spwn/spwn/services/client"
)

const projectFile = "spwn.toml"

type ProjectConfig struct {
	Project ProjectMeta `toml:"project"`
	VMs     []VMConfig  `toml:"vm"`
}

type ProjectMeta struct {
	Name string `toml:"name"`
}

type VMConfig struct {
	Name     string `toml:"name"`
	Image    string `toml:"image"`
	Vcpus    int64  `toml:"vcpus"`
	MemoryMb int    `toml:"memory_mb"`
	Port     int    `toml:"port"`
}

func loadProjectConfig() (*ProjectConfig, error) {
	data, err := os.ReadFile(projectFile)
	if err != nil {
		return nil, fmt.Errorf("no %s found — run 'spwn init' to create one", projectFile)
	}
	var cfg ProjectConfig
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("parse %s: %w", projectFile, err)
	}
	if len(cfg.VMs) == 0 {
		return nil, fmt.Errorf("%s has no [[vm]] entries", projectFile)
	}
	return &cfg, nil
}

// ── init ──────────────────────────────────────────────────────────────────────

func initCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "init",
		Short: "create a spwn.toml for this project",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if _, err := os.Stat(projectFile); err == nil {
				var overwrite bool
				if err := huh.NewConfirm().
					Title("spwn.toml already exists. Overwrite?").
					Affirmative("yes").
					Negative("cancel").
					Value(&overwrite).
					Run(); err != nil || !overwrite {
					fmt.Println("aborted")
					return nil
				}
			}

			var projectName, vmName, image string
			var vcpus int64 = 1000
			var memMb int = 512
			var port int = 8080

			if err := huh.NewForm(
				huh.NewGroup(
					huh.NewInput().
						Title("Project name").
						Value(&projectName),
					huh.NewInput().
						Title("VM name").
						Value(&vmName),
					huh.NewSelect[string]().
						Title("Base image").
						Options(
							huh.NewOption("ubuntu", "ubuntu"),
							huh.NewOption("debian", "debian"),
							huh.NewOption("alpine", "alpine"),
						).
						Value(&image),
				),
			).Run(); err != nil {
				return err
			}

			if projectName == "" {
				cwd, _ := os.Getwd()
				parts := strings.Split(cwd, "/")
				projectName = parts[len(parts)-1]
			}
			if vmName == "" {
				vmName = projectName
			}

			cfg := ProjectConfig{
				Project: ProjectMeta{Name: projectName},
				VMs: []VMConfig{
					{
						Name:     vmName,
						Image:    image,
						Vcpus:    vcpus,
						MemoryMb: memMb,
						Port:     port,
					},
				},
			}

			content := renderTOML(cfg)
			if err := os.WriteFile(projectFile, []byte(content), 0644); err != nil {
				return fmt.Errorf("write %s: %w", projectFile, err)
			}

			printOK(fmt.Sprintf("created %s", styleVal.Render(projectFile)))
			printHint("run 'spwn up' to create and start your VMs")
			return nil
		},
	}
}

func renderTOML(cfg ProjectConfig) string {
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("[project]\nname = %q\n", cfg.Project.Name))
	for _, vm := range cfg.VMs {
		sb.WriteString("\n[[vm]]\n")
		sb.WriteString(fmt.Sprintf("name      = %q\n", vm.Name))
		sb.WriteString(fmt.Sprintf("image     = %q\n", vm.Image))
		sb.WriteString(fmt.Sprintf("vcpus     = %d\n", vm.Vcpus))
		sb.WriteString(fmt.Sprintf("memory_mb = %d\n", vm.MemoryMb))
		sb.WriteString(fmt.Sprintf("port      = %d\n", vm.Port))
	}
	return sb.String()
}

// ── up ────────────────────────────────────────────────────────────────────────

func upCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "up",
		Short: "create and start VMs defined in spwn.toml",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadProjectConfig()
			if err != nil {
				return err
			}
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}

			for _, vmCfg := range cfg.VMs {
				vm, action, err := ensureVM(c, vmCfg)
				if err != nil {
					fmt.Fprintf(os.Stderr, "  %s %s: %v\n", styleDim.Render("✗"), vmCfg.Name, err)
					continue
				}
				_ = vm
				fmt.Printf("  %s %s  %s\n",
					styleVal.Render("✓"),
					styleVal.Render(vmCfg.Name),
					styleDim.Render(action),
				)
			}
			return nil
		},
	}
}

// ensureVM creates the VM if it doesn't exist, then starts it if stopped.
// Returns the final VM state and a short description of what happened.
func ensureVM(c *client.Client, cfg VMConfig) (client.VM, string, error) {
	existing, err := c.GetVMByName(cfg.Name)
	if err != nil {
		return client.VM{}, "", fmt.Errorf("lookup: %w", err)
	}

	var vm client.VM
	if len(existing) == 0 {
		vm, err = c.CreateVM(client.CreateVMRequest{
			Name:        cfg.Name,
			Image:       cfg.Image,
			Vcpus:       cfg.Vcpus,
			MemoryMb:    cfg.MemoryMb,
			ExposedPort: cfg.Port,
		})
		if err != nil {
			return client.VM{}, "", fmt.Errorf("create: %w", err)
		}
	} else {
		vm = existing[0]
	}

	switch vm.Status {
	case "running":
		return vm, "already running", nil
	case "starting":
		return vm, "starting", nil
	default:
		if err := c.StartVM(vm.ID); err != nil {
			return vm, "", fmt.Errorf("start: %w", err)
		}
		if len(existing) == 0 {
			return vm, "created + starting", nil
		}
		return vm, "starting", nil
	}
}

// ── down ──────────────────────────────────────────────────────────────────────

func downCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "down",
		Short: "stop VMs defined in spwn.toml",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadProjectConfig()
			if err != nil {
				return err
			}
			c, err := client.NewAuthedClient()
			if err != nil {
				return err
			}

			for _, vmCfg := range cfg.VMs {
				vms, err := c.GetVMByName(vmCfg.Name)
				if err != nil || len(vms) == 0 {
					fmt.Printf("  %s %s  %s\n",
						styleDim.Render("–"),
						vmCfg.Name,
						styleDim.Render("not found"),
					)
					continue
				}
				vm := vms[0]
				if vm.Status == "stopped" {
					fmt.Printf("  %s %s  %s\n",
						styleDim.Render("–"),
						vmCfg.Name,
						styleDim.Render("already stopped"),
					)
					continue
				}
				if err := c.StopVM(vm.ID); err != nil {
					fmt.Fprintf(os.Stderr, "  ✗ %s: %v\n", vmCfg.Name, err)
					continue
				}
				fmt.Printf("  %s %s  %s\n",
					styleVal.Render("✓"),
					styleVal.Render(vmCfg.Name),
					styleDim.Render("stopping"),
				)
			}
			return nil
		},
	}
}

// ── shell-prompt ──────────────────────────────────────────────────────────────

func shellPromptCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "shell-prompt",
		Short: "output a PS1 fragment showing project VM status",
		Long: `Outputs a compact status string for embedding in your shell prompt.

Add to your shell config:

  # bash / zsh
  PS1='$(spwn shell-prompt) \$ '

  # fish
  function fish_prompt
    echo (spwn shell-prompt) '> '
  end

Outputs nothing if no spwn.toml is found or the API is unreachable.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := loadProjectConfig()
			if err != nil {
				return nil // silent — don't break PS1
			}
			c, err := client.NewAuthedClient()
			if err != nil {
				return nil
			}

			var parts []string
			for _, vmCfg := range cfg.VMs {
				vms, err := c.GetVMByName(vmCfg.Name)
				if err != nil || len(vms) == 0 {
					parts = append(parts, vmCfg.Name+"?")
					continue
				}
				switch vms[0].Status {
				case "running":
					parts = append(parts, vmCfg.Name+"↑")
				case "starting":
					parts = append(parts, vmCfg.Name+"…")
				case "stopped":
					parts = append(parts, vmCfg.Name+"↓")
				default:
					parts = append(parts, vmCfg.Name+"?")
				}
			}

			if len(parts) > 0 {
				fmt.Printf("[spwn: %s]", strings.Join(parts, " "))
			}
			return nil
		},
	}
}
