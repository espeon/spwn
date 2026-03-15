package main

import (
	"fmt"
	"regexp"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/spwn/spwn/services/tui"
)

// ── messages ──────────────────────────────────────────────────────────────────

type gwVmsLoadedMsg struct{ vms []vmListItem }
type gwActionDoneMsg struct {
	err error
	msg string
}
type gwVmCreatedMsg struct{ vm vmListItem }

// ── views ─────────────────────────────────────────────────────────────────────

type gwView int

const (
	gwViewList gwView = iota
	gwViewNewVM
	gwViewConfirm
)

// ── model ─────────────────────────────────────────────────────────────────────

type GatewayApp struct {
	cfg       *gatewayConfig
	accountID string

	// result — set when user chooses to connect
	ConnectVMID string

	vms       []vmListItem
	cursor    int
	statusMsg string
	errMsg    string
	loading   bool

	width  int
	height int

	view gwView

	// new VM form
	newName  string
	newField int // 0=name

	// confirm dialog
	confirmMsg    string
	confirmAction func() tea.Cmd
}

func NewGatewayApp(cfg *gatewayConfig, accountID string, vms []vmListItem, width, height int) GatewayApp {
	return GatewayApp{
		cfg:       cfg,
		accountID: accountID,
		vms:       vms,
		width:     width,
		height:    height,
	}
}

func (m GatewayApp) Init() tea.Cmd { return nil }

func (m GatewayApp) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width, m.height = msg.Width, msg.Height
		return m, nil

	case gwVmsLoadedMsg:
		m.vms = msg.vms
		m.loading = false
		if m.cursor >= len(m.vms) && len(m.vms) > 0 {
			m.cursor = len(m.vms) - 1
		}
		return m, nil

	case gwActionDoneMsg:
		m.loading = false
		if msg.err != nil {
			m.errMsg = msg.err.Error()
		} else {
			m.statusMsg = msg.msg
			m.errMsg = ""
		}
		return m, m.reloadVMs()

	case gwVmCreatedMsg:
		m.loading = false
		m.statusMsg = "created " + msg.vm.Name
		m.errMsg = ""
		m.view = gwViewList
		return m, m.reloadVMs()
	}

	switch m.view {
	case gwViewList:
		return m.updateList(msg)
	case gwViewNewVM:
		return m.updateNewVM(msg)
	case gwViewConfirm:
		return m.updateConfirm(msg)
	}
	return m, nil
}

func (m GatewayApp) reloadVMs() tea.Cmd {
	cfg := m.cfg
	accountID := m.accountID
	return func() tea.Msg {
		vms, err := cfg.listAccountVMs(accountID)
		if err != nil {
			return gwActionDoneMsg{err: err}
		}
		return gwVmsLoadedMsg{vms}
	}
}

func (m GatewayApp) updateList(msg tea.Msg) (tea.Model, tea.Cmd) {
	km, ok := msg.(tea.KeyMsg)
	if !ok {
		return m, nil
	}
	switch km.String() {
	case "q", "ctrl+c":
		return m, tea.Quit

	case "up", "k":
		if m.cursor > 0 {
			m.cursor--
		}

	case "down", "j":
		if m.cursor < len(m.vms)-1 {
			m.cursor++
		}

	case "enter":
		if len(m.vms) == 0 {
			break
		}
		vm := m.vms[m.cursor]
		if vm.Status != "running" {
			m.errMsg = vm.Name + " is not running"
			break
		}
		m.ConnectVMID = vm.ID
		return m, tea.Quit

	case "s":
		if len(m.vms) == 0 {
			break
		}
		vm := m.vms[m.cursor]
		m.loading = true
		m.statusMsg = ""
		m.errMsg = ""
		cfg := m.cfg
		accountID := m.accountID
		var action func() tea.Msg
		if vm.Status == "running" {
			action = func() tea.Msg {
				err := cfg.stopVM(vm.ID, accountID)
				return gwActionDoneMsg{err: err, msg: "stopped " + vm.Name}
			}
		} else if vm.Status == "stopped" {
			action = func() tea.Msg {
				err := cfg.startVM(vm.ID, accountID)
				return gwActionDoneMsg{err: err, msg: "started " + vm.Name}
			}
		} else {
			m.errMsg = vm.Name + " is " + vm.Status
			break
		}
		return m, action

	case "n":
		m.view = gwViewNewVM
		m.newName = ""
		m.newField = 0

	case "d":
		if len(m.vms) == 0 {
			break
		}
		vm := m.vms[m.cursor]
		m.confirmMsg = "delete " + vm.Name + "?"
		m.confirmAction = func() tea.Cmd {
			cfg := m.cfg
			accountID := m.accountID
			return func() tea.Msg {
				err := cfg.deleteVM(vm.ID, accountID)
				return gwActionDoneMsg{err: err, msg: "deleted " + vm.Name}
			}
		}
		m.view = gwViewConfirm
	}

	return m, nil
}

func (m GatewayApp) updateNewVM(msg tea.Msg) (tea.Model, tea.Cmd) {
	km, ok := msg.(tea.KeyMsg)
	if !ok {
		return m, nil
	}
	switch {
	case km.String() == "esc":
		m.view = gwViewList
	case km.String() == "enter":
		if m.newName == "" {
			break
		}
		name := m.newName
		cfg := m.cfg
		accountID := m.accountID
		m.loading = true
		m.view = gwViewList
		return m, func() tea.Msg {
			vm, err := cfg.createVM(accountID, createVMRequest{Name: name})
			if err != nil {
				return gwActionDoneMsg{err: err}
			}
			return gwVmCreatedMsg{vm}
		}
	case km.Type == tea.KeyBackspace:
		if len(m.newName) > 0 {
			m.newName = m.newName[:len(m.newName)-1]
		}
	default:
		if ch := km.String(); len(ch) == 1 {
			m.newName += ch
		}
	}
	return m, nil
}

func (m GatewayApp) updateConfirm(msg tea.Msg) (tea.Model, tea.Cmd) {
	km, ok := msg.(tea.KeyMsg)
	if !ok {
		return m, nil
	}
	switch km.String() {
	case "y", "Y":
		action := m.confirmAction
		m.confirmAction = nil
		m.view = gwViewList
		return m, action()
	case "n", "N", "esc":
		m.view = gwViewList
	}
	return m, nil
}

// ── views ─────────────────────────────────────────────────────────────────────

func (m GatewayApp) View() string {
	switch m.view {
	case gwViewNewVM:
		return m.newVMView()
	case gwViewConfirm:
		return m.confirmView()
	default:
		return m.listView()
	}
}

func (m GatewayApp) listView() string {
	w := m.width
	if w == 0 {
		w = 80
	}
	h := m.height
	if h == 0 {
		h = 24
	}

	sep := dimStyle(strings.Repeat("─", w))
	header := headerStyle("spwn")
	hint := "↑/↓  enter:connect  s:start/stop  n:new  d:delete  q:quit"

	var body strings.Builder
	if m.loading {
		body.WriteString(pad("  "+dimStyle("…"), w) + "\n")
	} else if len(m.vms) == 0 {
		body.WriteString(pad("  "+dimStyle("no VMs — press n to create one"), w) + "\n")
	} else {
		for i, vm := range m.vms {
			dot := tui.StatusDot(vm.Status)
			status := statusStyle(vm.Status, fmt.Sprintf("%-10s", vm.Status))
			sub := ""
			if vm.Subdomain != "" {
				sub = "  " + dimStyle(vm.Subdomain)
			}
			line := fmt.Sprintf("%s  %s  %s%s", dot, vm.Name, status, sub)
			var row string
			if i == m.cursor {
				row = "  " + selectedStyle("> "+line)
			} else {
				row = "    " + line
			}
			body.WriteString(pad(row, w) + "\n")
		}
	}

	// status / error bar (1 line, empty string = no bar)
	var statusLine string
	if m.errMsg != "" {
		statusLine = pad("  "+errorStyle(m.errMsg), w) + "\n"
	} else if m.statusMsg != "" {
		statusLine = pad("  "+dimStyle(m.statusMsg), w) + "\n"
	}

	// Layout:
	//   header(1) + sep(1) + blank(1)  = 3 top
	//   body lines                      = bodyLines
	//   padding                         = padding
	//   statusLine                      = 0 or 1
	//   sep(1) + hint(1)               = 2 bottom
	// total = 3 + bodyLines + padding + statusLines + 2 = h
	bodyLines := strings.Count(body.String(), "\n")
	statusLines := 0
	if statusLine != "" {
		statusLines = 1
	}
	padding := h - 5 - bodyLines - statusLines
	if padding < 0 {
		padding = 0
	}

	var out strings.Builder
	out.WriteString(header + "\n")
	out.WriteString(sep + "\n")
	out.WriteString("\n")
	out.WriteString(body.String())
	for i := 0; i < padding; i++ {
		out.WriteString("\n")
	}
	out.WriteString(statusLine)
	out.WriteString(sep + "\n")
	out.WriteString("  " + dimStyle(hint))
	return out.String()
}

func (m GatewayApp) newVMView() string {
	w := m.width
	if w == 0 {
		w = 80
	}
	sep := dimStyle(strings.Repeat("─", w))
	var out strings.Builder
	out.WriteString(headerStyle("spwn") + dimStyle(" / new VM") + "\n")
	out.WriteString(sep + "\n\n")
	out.WriteString("  Name:  " + selectedStyle(m.newName+"█") + "\n\n")
	out.WriteString(sep + "\n")
	out.WriteString("  " + dimStyle("enter to create · esc to cancel"))
	return out.String()
}

func (m GatewayApp) confirmView() string {
	w := m.width
	if w == 0 {
		w = 80
	}
	sep := dimStyle(strings.Repeat("─", w))
	var out strings.Builder
	out.WriteString(headerStyle("spwn") + "\n")
	out.WriteString(sep + "\n\n")
	out.WriteString("  " + headerStyle(m.confirmMsg) + "\n\n")
	out.WriteString(sep + "\n")
	out.WriteString("  " + dimStyle("y to confirm · n/esc to cancel"))
	return out.String()
}

// ── style + layout helpers ────────────────────────────────────────────────────

var ansiEscape = regexp.MustCompile(`\x1b\[[0-9;]*m`)

// visibleLen returns the printable length of s, ignoring ANSI escape codes.
func visibleLen(s string) int {
	return len([]rune(ansiEscape.ReplaceAllString(s, "")))
}

// pad returns s padded with spaces to exactly w visible columns.
func pad(s string, w int) string {
	vl := visibleLen(s)
	if vl >= w {
		return s
	}
	return s + strings.Repeat(" ", w-vl)
}

// raw ANSI to avoid duplicating lipgloss setup from the tui package

func dimStyle(s string) string      { return "\x1b[38;5;242m" + s + "\x1b[0m" }
func selectedStyle(s string) string { return "\x1b[1;38;5;111m" + s + "\x1b[0m" }
func errorStyle(s string) string    { return "\x1b[38;5;203m" + s + "\x1b[0m" }
func headerStyle(s string) string   { return "\x1b[1;38;5;189m" + s + "\x1b[0m" }

func statusStyle(status, s string) string {
	switch status {
	case "running":
		return "\x1b[38;5;114m" + s + "\x1b[0m"
	case "starting", "stopping":
		return "\x1b[38;5;222m" + s + "\x1b[0m"
	case "error":
		return "\x1b[38;5;210m" + s + "\x1b[0m"
	default:
		return "\x1b[38;5;242m" + s + "\x1b[0m"
	}
}
