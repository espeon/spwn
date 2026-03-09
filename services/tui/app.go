package tui

import (
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/spwn/spwn/services/client"
)

type view int

const (
	viewVMList view = iota
	viewVMDetail
	viewSnapshotDetail
	viewAccount
	viewNewVM
	viewConfirm
	viewRename
)

type tickMsg struct{}
type vmsLoadedMsg struct{ vms []client.VM }
type vmDetailLoadedMsg struct {
	vm     client.VM
	events []client.VMEvent
	snaps  []client.Snapshot
}
type accountLoadedMsg struct{ account client.Account }
type errMsg struct{ err error }
type actionDoneMsg struct{ err error }
type vmCreatedMsg struct{ vm client.VM }

func tickCmd() tea.Cmd {
	return tea.Tick(5*time.Second, func(time.Time) tea.Msg {
		return tickMsg{}
	})
}

type App struct {
	client  *client.Client
	current view
	width   int
	height  int

	// VM list state
	vmList    []client.VM
	vmCursor  int
	vmLoading bool
	vmErr     string

	// VM detail state
	detailVM     *client.VM
	detailEvents []client.VMEvent
	detailSnaps  []client.Snapshot
	detailCursor int // cursor over snapshots list

	// Snapshot detail state
	selectedSnap *client.Snapshot

	// Account state
	account *client.Account

	// New VM form state
	newVMName   string
	newVMVcpus  string
	newVMMemMb  string
	newVMField  int // 0=name, 1=vcpus, 2=mem

	// Confirm dialog state
	confirmMsg    string
	confirmAction func() tea.Cmd

	// Rename state
	renameValue string

	// Status message (bottom of screen)
	statusMsg string

	showHelp bool
}

func Run(c *client.Client) error {
	app := &App{
		client:      c,
		current:     viewVMList,
		newVMVcpus: "2",
		newVMMemMb:  "512",
	}
	p := tea.NewProgram(app, tea.WithAltScreen())
	_, err := p.Run()
	return err
}

func (a *App) Init() tea.Cmd {
	return tea.Batch(a.loadVMs(), tickCmd())
}

func (a *App) loadVMs() tea.Cmd {
	return func() tea.Msg {
		vms, err := a.client.ListVMs()
		if err != nil {
			return errMsg{err}
		}
		return vmsLoadedMsg{vms}
	}
}

func (a *App) loadVMDetail(vm client.VM) tea.Cmd {
	return func() tea.Msg {
		events, _ := a.client.ListVMEvents(vm.ID, 10, nil)
		snaps, _ := a.client.ListSnapshots(vm.ID)
		return vmDetailLoadedMsg{vm: vm, events: events, snaps: snaps}
	}
}

func (a *App) loadAccount() tea.Cmd {
	return func() tea.Msg {
		acc, err := a.client.Me()
		if err != nil {
			return errMsg{err}
		}
		return accountLoadedMsg{acc}
	}
}

func (a *App) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		a.width, a.height = msg.Width, msg.Height
		return a, nil

	case tickMsg:
		if a.current == viewVMList {
			return a, tea.Batch(a.loadVMs(), tickCmd())
		}
		return a, tickCmd()

	case vmsLoadedMsg:
		a.vmList = msg.vms
		a.vmLoading = false
		a.vmErr = ""
		if a.vmCursor >= len(a.vmList) && len(a.vmList) > 0 {
			a.vmCursor = len(a.vmList) - 1
		}
		return a, nil

	case vmDetailLoadedMsg:
		a.detailVM = &msg.vm
		a.detailEvents = msg.events
		a.detailSnaps = msg.snaps
		return a, nil

	case accountLoadedMsg:
		a.account = &msg.account
		return a, nil

	case errMsg:
		a.vmErr = msg.err.Error()
		return a, nil

	case actionDoneMsg:
		if msg.err != nil {
			a.statusMsg = "error: " + msg.err.Error()
		} else {
			a.statusMsg = ""
		}
		return a, a.loadVMs()

	case vmCreatedMsg:
		a.current = viewVMList
		a.statusMsg = "created " + msg.vm.Name
		return a, a.loadVMs()
	}

	switch a.current {
	case viewVMList:
		return a.updateVMList(msg)
	case viewVMDetail:
		return a.updateVMDetail(msg)
	case viewSnapshotDetail:
		return a.updateSnapshotDetail(msg)
	case viewAccount:
		return a.updateAccount(msg)
	case viewNewVM:
		return a.updateNewVM(msg)
	case viewConfirm:
		return a.updateConfirm(msg)
	case viewRename:
		return a.updateRename(msg)
	}
	return a, nil
}

func (a *App) View() string {
	if a.showHelp {
		return a.helpView()
	}
	switch a.current {
	case viewVMList:
		return a.vmListView()
	case viewVMDetail:
		return a.vmDetailView()
	case viewSnapshotDetail:
		return a.snapshotDetailView()
	case viewAccount:
		return a.accountView()
	case viewNewVM:
		return a.newVMView()
	case viewConfirm:
		return a.confirmView()
	case viewRename:
		return a.renameView()
	}
	return ""
}
