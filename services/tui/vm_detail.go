package tui

import (
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/key"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/spwn/spwn/services/client"
)

func (a *App) updateVMDetail(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch {
		case key.Matches(msg, keys.Esc):
			a.current = viewVMList
			return a, a.loadVMs()
		case key.Matches(msg, keys.Up):
			if a.detailCursor > 0 {
				a.detailCursor--
			}
		case key.Matches(msg, keys.Down):
			if a.detailCursor < len(a.detailSnaps)-1 {
				a.detailCursor++
			}
		case key.Matches(msg, keys.Enter):
			if len(a.detailSnaps) > 0 {
				snap := a.detailSnaps[a.detailCursor]
				a.selectedSnap = &snap
				a.current = viewSnapshotDetail
			}
		case key.Matches(msg, keys.Connect):
			if a.detailVM != nil && a.connectFn != nil {
				if a.detailVM.Status == "running" {
					return a, a.connectCmd(a.detailVM.ID)
				}
				a.statusMsg = a.detailVM.Name + " is not running"
			}
		case key.Matches(msg, keys.Start):
			if a.detailVM != nil {
				return a, a.toggleVM(*a.detailVM)
			}
		case key.Matches(msg, keys.Snap):
			if a.detailVM != nil {
				vm := *a.detailVM
				return a, func() tea.Msg {
					_, err := a.client.TakeSnapshot(vm.ID, nil)
					if err != nil {
						return actionDoneMsg{err}
					}
					return actionDoneMsg{}
				}
			}
		case key.Matches(msg, keys.Rename):
			if a.detailVM != nil {
				a.renameValue = a.detailVM.Name
				a.current = viewRename
			}
		case key.Matches(msg, keys.Delete):
			if a.detailVM != nil {
				vm := *a.detailVM
				a.confirmMsg = fmt.Sprintf("delete %s?", vm.Name)
				a.confirmAction = func() tea.Cmd {
					return func() tea.Msg {
						err := a.client.DeleteVM(vm.ID)
						if err == nil {
							return actionDoneMsg{}
						}
						return actionDoneMsg{err}
					}
				}
				a.current = viewConfirm
			}
		case key.Matches(msg, keys.Help):
			a.showHelp = true
		}
	}
	return a, nil
}

func (a *App) vmDetailView() string {
	if a.detailVM == nil {
		return styleDim.Render("loading…")
	}
	vm := *a.detailVM
	var b strings.Builder

	crumb := styleHeader.Render("spwn") + styleDim.Render(" / ") + styleTitle.Render(vm.Name)
	fmt.Fprintln(&b, crumb)
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteByte('\n')

	fmt.Fprintf(&b, "  %s %s   %dvc   %dMB   %s\n\n",
		statusDot(vm.Status),
		statusColor(vm.Status).Render(vm.Status),
		vm.Vcpus, vm.MemoryMb,
		styleDim.Render(vm.Subdomain),
	)

	// Two-column: events + snapshots
	eventsCol := a.eventsColumn()
	snapsCol := a.snapshotsColumn()

	colWidth := (a.width - 6) / 2
	leftPad := fmt.Sprintf("%-*s", colWidth, eventsCol)
	_ = leftPad

	fmt.Fprintf(&b, "  %s%s%s\n",
		styleTitle.Render("Recent events"),
		strings.Repeat(" ", colWidth-len("Recent events")-2),
		styleTitle.Render("Snapshots"))
	fmt.Fprintf(&b, "  %s  %s\n",
		styleDim.Render(strings.Repeat("─", colWidth-2)),
		styleDim.Render(strings.Repeat("─", colWidth-2)))

	evLines := strings.Split(eventsCol, "\n")
	snLines := strings.Split(snapsCol, "\n")
	maxLines := len(evLines)
	if len(snLines) > maxLines {
		maxLines = len(snLines)
	}
	for i := 0; i < maxLines; i++ {
		left, right := "", ""
		if i < len(evLines) {
			left = evLines[i]
		}
		if i < len(snLines) {
			right = snLines[i]
		}
		fmt.Fprintf(&b, "  %-*s  %s\n", colWidth-2, left, right)
	}

	b.WriteByte('\n')
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	hint := "[s] start/stop  [p] snapshot  [r] rename  [d] delete  [enter] snap detail  [esc] back"
	if a.connectFn != nil {
		hint = "[c] connect  [s] start/stop  [p] snapshot  [r] rename  [d] delete  [enter] snap detail  [esc] back"
	}
	b.WriteString(styleDim.Render(hint))

	return b.String()
}

func (a *App) eventsColumn() string {
	if len(a.detailEvents) == 0 {
		return styleDim.Render("no events")
	}
	var lines []string
	for _, e := range a.detailEvents {
		age := timeAgo(e.CreatedAt)
		lines = append(lines, fmt.Sprintf("%-18s %s", e.Event, styleDim.Render(age)))
	}
	return strings.Join(lines, "\n")
}

func (a *App) snapshotsColumn() string {
	if len(a.detailSnaps) == 0 {
		return styleDim.Render("no snapshots")
	}
	var lines []string
	for i, s := range a.detailSnaps {
		label := ""
		if s.Label != nil {
			label = *s.Label
		} else {
			label = s.ID[:8]
		}
		age := timeAgo(s.CreatedAt)
		cursor := "  "
		style := styleDim
		if i == a.detailCursor {
			cursor = "▶ "
			style = styleSelected
		}
		lines = append(lines, cursor+style.Render(fmt.Sprintf("%-12s", label))+
			styleDim.Render(age))
	}
	return strings.Join(lines, "\n")
}

func timeAgo(unixSec int64) string {
	d := time.Since(time.Unix(unixSec, 0))
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d.Minutes()))
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	default:
		return fmt.Sprintf("%dd ago", int(d.Hours()/24))
	}
}

func (a *App) updateSnapshotDetail(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch {
		case key.Matches(msg, keys.Esc):
			a.current = viewVMDetail
		case key.Matches(msg, keys.Rename): // [r] = restore
			if a.selectedSnap != nil && a.detailVM != nil {
				vm := *a.detailVM
				snap := *a.selectedSnap
				a.confirmMsg = fmt.Sprintf("restore %s from %s?", vm.Name, snap.ID[:8])
				a.confirmAction = func() tea.Cmd {
					return func() tea.Msg {
						err := a.client.RestoreSnapshot(vm.ID, snap.ID)
						return actionDoneMsg{err}
					}
				}
				a.current = viewConfirm
			}
		case key.Matches(msg, keys.Delete):
			if a.selectedSnap != nil && a.detailVM != nil {
				vm := *a.detailVM
				snap := *a.selectedSnap
				a.confirmMsg = fmt.Sprintf("delete snapshot %s?", snap.ID[:8])
				a.confirmAction = func() tea.Cmd {
					return func() tea.Msg {
						err := a.client.DeleteSnapshot(vm.ID, snap.ID)
						if err == nil {
							return actionDoneMsg{}
						}
						return actionDoneMsg{err}
					}
				}
				a.current = viewConfirm
			}
		}
	}
	return a, nil
}

func (a *App) snapshotDetailView() string {
	if a.selectedSnap == nil || a.detailVM == nil {
		return styleDim.Render("loading…")
	}
	snap := *a.selectedSnap
	vm := *a.detailVM
	var b strings.Builder

	crumb := styleHeader.Render("spwn") +
		styleDim.Render(" / ") + styleTitle.Render(vm.Name) +
		styleDim.Render(" / ") + styleTitle.Render(snap.ID[:8])
	fmt.Fprintln(&b, crumb)
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteByte('\n')

	label := "(no label)"
	if snap.Label != nil {
		label = *snap.Label
	}

	fmt.Fprintf(&b, "  label      %s\n", label)
	fmt.Fprintf(&b, "  taken      %s  (%s)\n",
		timeAgo(snap.CreatedAt),
		time.Unix(snap.CreatedAt, 0).Format("2006-01-02 15:04"))
	fmt.Fprintf(&b, "  size       %.1f MB\n", float64(snap.SizeBytes)/1024/1024)

	b.WriteByte('\n')
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteString(styleDim.Render("[r] restore  [d] delete  [esc] back"))

	return b.String()
}

func (a *App) updateAccount(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		if key.Matches(msg, keys.Esc) || key.Matches(msg, keys.Quit) {
			a.current = viewVMList
		}
	}
	return a, nil
}

func (a *App) accountView() string {
	var b strings.Builder
	fmt.Fprintf(&b, "%s%s%s\n",
		styleHeader.Render("spwn"), styleDim.Render(" / "), styleTitle.Render("account"))
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteByte('\n')

	if a.account == nil {
		fmt.Fprintf(&b, "  %s\n", styleDim.Render("loading…"))
	} else {
		acc := *a.account
		fmt.Fprintf(&b, "  %s\n\n", styleTitle.Render(acc.Email))
		fmt.Fprintf(&b, "  %s\n", progressBar("vcpus", 0, int(acc.VcpuLimit/1000), a.width-4))
		fmt.Fprintf(&b, "  %s\n", progressBar("ram", 0, acc.MemLimitMb/1024, a.width-4))
		fmt.Fprintf(&b, "  %s\n", progressBar("vms", 0, acc.VmLimit, a.width-4))
	}

	b.WriteByte('\n')
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteString(styleDim.Render("[esc] back"))
	return b.String()
}

func progressBar(label string, used, total, width int) string {
	barWidth := 20
	filled := 0
	if total > 0 {
		filled = barWidth * used / total
	}
	bar := strings.Repeat("█", filled) + strings.Repeat("░", barWidth-filled)
	return fmt.Sprintf("%-8s %s  %d/%d",
		label,
		styleSelected.Render(bar),
		used, total)
}

func (a *App) updateNewVM(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch {
		case key.Matches(msg, keys.Esc):
			a.current = viewVMList
		case msg.String() == "tab" || key.Matches(msg, keys.Down):
			a.newVMField = (a.newVMField + 1) % 3
		case key.Matches(msg, keys.Up):
			a.newVMField = (a.newVMField + 2) % 3
		case key.Matches(msg, keys.Enter):
			if a.newVMField < 2 {
				a.newVMField++
			} else {
				return a, a.submitNewVM()
			}
		case msg.Type == tea.KeyBackspace:
			switch a.newVMField {
			case 0:
				if len(a.newVMName) > 0 {
					a.newVMName = a.newVMName[:len(a.newVMName)-1]
				}
			case 1:
				if len(a.newVMVcpus) > 0 {
					a.newVMVcpus = a.newVMVcpus[:len(a.newVMVcpus)-1]
				}
			case 2:
				if len(a.newVMMemMb) > 0 {
					a.newVMMemMb = a.newVMMemMb[:len(a.newVMMemMb)-1]
				}
			}
		default:
			ch := msg.String()
			if len(ch) == 1 {
				switch a.newVMField {
				case 0:
					a.newVMName += ch
				case 1:
					if ch >= "0" && ch <= "9" {
						a.newVMVcpus += ch
					}
				case 2:
					if ch >= "0" && ch <= "9" {
						a.newVMMemMb += ch
					}
				}
			}
		}
	}
	return a, nil
}

func (a *App) submitNewVM() tea.Cmd {
	name := a.newVMName
	vcpus := int64(parseInt(a.newVMVcpus, 1000))
	memMb := parseInt(a.newVMMemMb, 512)
	return func() tea.Msg {
		vm, err := a.client.CreateVM(client.CreateVMRequest{
			Name:     name,
			Vcpus:    vcpus,
			MemoryMb: memMb,
		})
		if err != nil {
			return actionDoneMsg{err}
		}
		return vmCreatedMsg{vm}
	}
}

func (a *App) newVMView() string {
	var b strings.Builder
	fmt.Fprintln(&b, styleHeader.Render("spwn")+styleDim.Render(" / new VM"))
	fmt.Fprintf(&b, "%s\n\n", styleDim.Render(strings.Repeat("─", a.width)))

	fields := []struct {
		label string
		value *string
		idx   int
	}{
		{"Name:   ", &a.newVMName, 0},
		{"vCPUs(m):", &a.newVMVcpus, 1},
		{"Memory: ", &a.newVMMemMb, 2},
	}

	for _, f := range fields {
		cursor := " "
		style := styleDim
		if f.idx == a.newVMField {
			cursor = "▶"
			style = styleSelected
		}
		val := *f.value
		if f.idx == a.newVMField {
			val += "█"
		}
		fmt.Fprintf(&b, "  %s %s%s\n", cursor, f.label, style.Render(val))
	}

	b.WriteByte('\n')
	fmt.Fprintln(&b, styleDim.Render(strings.Repeat("─", a.width)))
	b.WriteString(styleDim.Render("[tab/↑↓] next field  [enter] create  [esc] cancel"))
	return b.String()
}

func (a *App) updateConfirm(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "y", "Y":
			action := a.confirmAction
			a.current = viewVMList
			a.confirmAction = nil
			return a, action()
		case "n", "N", "esc":
			a.current = viewVMList
		}
	}
	return a, nil
}

func (a *App) confirmView() string {
	var b strings.Builder
	fmt.Fprintln(&b, styleHeader.Render("spwn"))
	fmt.Fprintf(&b, "%s\n\n", styleDim.Render(strings.Repeat("─", a.width)))
	fmt.Fprintf(&b, "  %s\n\n", styleTitle.Render(a.confirmMsg))
	fmt.Fprintf(&b, "  %s", styleDim.Render("[y] confirm  [n/esc] cancel"))
	return b.String()
}

func (a *App) updateRename(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch {
		case key.Matches(msg, keys.Esc):
			a.current = viewVMDetail
		case key.Matches(msg, keys.Enter):
			if a.detailVM != nil && a.renameValue != "" {
				vm := *a.detailVM
				newName := a.renameValue
				a.current = viewVMDetail
				return a, func() tea.Msg {
					updated, err := a.client.PatchVM(vm.ID, client.PatchVMRequest{Name: &newName})
					if err != nil {
						return actionDoneMsg{err}
					}
					a.detailVM = &updated
					return actionDoneMsg{}
				}
			}
		case msg.Type == tea.KeyBackspace:
			if len(a.renameValue) > 0 {
				a.renameValue = a.renameValue[:len(a.renameValue)-1]
			}
		default:
			if ch := msg.String(); len(ch) == 1 {
				a.renameValue += ch
			}
		}
	}
	return a, nil
}

func (a *App) renameView() string {
	var b strings.Builder
	fmt.Fprintln(&b, styleHeader.Render("spwn")+styleDim.Render(" / rename"))
	fmt.Fprintf(&b, "%s\n\n", styleDim.Render(strings.Repeat("─", a.width)))
	fmt.Fprintf(&b, "  Name: %s\n\n", styleSelected.Render(a.renameValue+"█"))
	b.WriteString(styleDim.Render("[enter] confirm  [esc] cancel"))
	return b.String()
}

func parseInt(s string, def int) int {
	n := 0
	for _, c := range s {
		if c < '0' || c > '9' {
			return def
		}
		n = n*10 + int(c-'0')
	}
	if n == 0 {
		return def
	}
	return n
}

func parseFloat(s string, def float64) float64 {
	if s == "" {
		return def
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil || f <= 0 {
		return def
	}
	return f
}
