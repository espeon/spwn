package tui

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/bubbles/key"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/spwn/spwn/services/client"
)

func (a *App) updateVMList(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		if a.showHelp {
			a.showHelp = false
			return a, nil
		}
		switch {
		case key.Matches(msg, keys.Quit):
			return a, tea.Quit
		case key.Matches(msg, keys.Up):
			if a.vmCursor > 0 {
				a.vmCursor--
			}
		case key.Matches(msg, keys.Down):
			if a.vmCursor < len(a.vmList)-1 {
				a.vmCursor++
			}
		case key.Matches(msg, keys.Enter):
			if len(a.vmList) > 0 {
				vm := a.vmList[a.vmCursor]
				a.current = viewVMDetail
				a.detailVM = &vm
				a.detailCursor = 0
				return a, a.loadVMDetail(vm)
			}
		case key.Matches(msg, keys.Start):
			if len(a.vmList) > 0 {
				vm := a.vmList[a.vmCursor]
				return a, a.toggleVM(vm)
			}
		case key.Matches(msg, keys.New):
			a.current = viewNewVM
			a.newVMName = ""
			a.newVMVcpus = "2"
			a.newVMMemMb = "512"
			a.newVMField = 0
		case key.Matches(msg, keys.Delete):
			if len(a.vmList) > 0 {
				vm := a.vmList[a.vmCursor]
				a.confirmMsg = fmt.Sprintf("delete %s?", vm.Name)
				a.confirmAction = func() tea.Cmd {
					return func() tea.Msg {
						err := a.client.DeleteVM(vm.ID)
						return actionDoneMsg{err}
					}
				}
				a.current = viewConfirm
			}
		case key.Matches(msg, keys.Account):
			a.current = viewAccount
			return a, a.loadAccount()
		case key.Matches(msg, keys.Help):
			a.showHelp = true
		}
	}
	return a, nil
}

func (a *App) toggleVM(vm client.VM) tea.Cmd {
	return func() tea.Msg {
		var err error
		if vm.Status == "running" {
			err = a.client.StopVM(vm.ID)
		} else if vm.Status == "stopped" {
			err = a.client.StartVM(vm.ID)
		}
		return actionDoneMsg{err}
	}
}

func (a *App) vmListView() string {
	var b strings.Builder

	// Header
	header := styleHeader.Render("spwn")
	b.WriteString(header + "\n")
	b.WriteString(styleDim.Render(strings.Repeat("─", a.width)) + "\n")

	if a.vmLoading && len(a.vmList) == 0 {
		b.WriteString("\n  " + styleDim.Render("loading…") + "\n")
	} else if a.vmErr != "" {
		b.WriteString("\n  " + styleError.Render(a.vmErr) + "\n")
	} else if len(a.vmList) == 0 {
		b.WriteString("\n  " + styleDim.Render("no VMs — press n to create one") + "\n")
	} else {
		b.WriteString("\n")
		for i, vm := range a.vmList {
			cursor := "  "
			nameStyle := lipgloss.NewStyle()
			if i == a.vmCursor {
				cursor = "▶ "
				nameStyle = styleSelected
			}
			dot := statusDot(vm.Status)
			name := nameStyle.Render(vm.Name)
			status := statusColor(vm.Status).Render(fmt.Sprintf("%-10s", vm.Status))
			res := styleDim.Render(fmt.Sprintf("%dvc  %dMB", vm.Vcpus, vm.MemoryMb))
			sub := styleDim.Render(vm.Subdomain)
			b.WriteString(fmt.Sprintf("  %s%s %s  %s  %s  %s\n",
				cursor, dot, name, status, res, sub))
		}
		b.WriteString("\n")
	}

	b.WriteString(styleDim.Render(strings.Repeat("─", a.width)) + "\n")

	if a.statusMsg != "" {
		b.WriteString(styleStatusBar.Render(a.statusMsg) + "\n")
	}

	b.WriteString(styleDim.Render(
		"[↑↓/jk] nav  [enter] detail  [s] start/stop  [n] new  [d] delete  [a] account  [?] help  [q] quit",
	))

	return b.String()
}

func (a *App) helpView() string {
	var b strings.Builder
	b.WriteString(styleHeader.Render("spwn — key bindings") + "\n\n")
	bindings := []struct{ key, desc string }{
		{"j / ↓", "move down"},
		{"k / ↑", "move up"},
		{"enter", "select / detail"},
		{"esc", "back / cancel"},
		{"s", "start / stop VM"},
		{"n", "new VM"},
		{"d", "delete VM"},
		{"r", "rename VM"},
		{"p", "take snapshot"},
		{"a", "account view"},
		{"?", "toggle help"},
		{"q", "quit"},
	}
	for _, b2 := range bindings {
		b.WriteString(fmt.Sprintf("  %-12s  %s\n",
			styleSelected.Render(b2.key), b2.desc))
	}
	b.WriteString("\n" + styleDim.Render("press any key to close"))
	return b.String()
}
