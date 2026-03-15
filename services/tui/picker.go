package tui

import (
	"fmt"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
)

// PickerItem is a minimal VM descriptor used by the SSH gateway VM picker.
type PickerItem struct {
	ID        string
	Name      string
	Status    string
	Subdomain string
}

// Picker is a standalone bubbletea model for selecting a VM in an SSH session.
type Picker struct {
	Items    []PickerItem
	cursor   int
	Chosen   *PickerItem
	Quitting bool
}

func NewPicker(items []PickerItem) Picker {
	return Picker{Items: items}
}

func (m Picker) Init() tea.Cmd { return nil }

func (m Picker) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "up", "k":
			if m.cursor > 0 {
				m.cursor--
			}
		case "down", "j":
			if m.cursor < len(m.Items)-1 {
				m.cursor++
			}
		case "enter":
			if len(m.Items) > 0 {
				item := m.Items[m.cursor]
				m.Chosen = &item
			}
			return m, tea.Quit
		case "q", "ctrl+c":
			m.Quitting = true
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m Picker) View() string {
	if m.Quitting || len(m.Items) == 0 {
		return ""
	}
	var b strings.Builder
	b.WriteString("\n  your vms:\n\n")
	for i, item := range m.Items {
		dot := statusDot(item.Status)
		sub := ""
		if item.Subdomain != "" {
			sub = "  " + styleDim.Render(item.Subdomain)
		}
		line := fmt.Sprintf("%s  %s%s", dot, item.Name, sub)
		if i == m.cursor {
			b.WriteString("  " + styleSelected.Render("> "+line) + "\n")
		} else {
			b.WriteString("    " + line + "\n")
		}
	}
	b.WriteString("\n  " + styleDim.Render("↑/↓ · enter to connect · q to quit") + "\n")
	return b.String()
}
