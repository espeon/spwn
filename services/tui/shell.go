package tui

import (
	"io"

	tea "github.com/charmbracelet/bubbletea"
)

// shellRelay implements tea.ExecCommand, letting bubbletea suspend itself while
// the gRPC console relay runs, then resume when the user exits the shell.
type shellRelay struct {
	vmID      string
	connectFn ConnectFn
	stdin     io.Reader
	stdout    io.Writer
	stderr    io.Writer
}

func (r *shellRelay) SetStdin(in io.Reader)  { r.stdin = in }
func (r *shellRelay) SetStdout(out io.Writer) { r.stdout = out }
func (r *shellRelay) SetStderr(err io.Writer) { r.stderr = err }

func (r *shellRelay) Run() error {
	return r.connectFn(r.vmID, r.stdin, r.stdout)
}

// connectCmd returns a tea.Cmd that suspends the TUI and relays a shell
// session to vmID, then resumes and emits shellDoneMsg.
func (a *App) connectCmd(vmID string) tea.Cmd {
	if a.connectFn == nil {
		return func() tea.Msg {
			return shellDoneMsg{err: nil}
		}
	}
	return tea.Exec(&shellRelay{vmID: vmID, connectFn: a.connectFn}, func(err error) tea.Msg {
		return shellDoneMsg{err: err}
	})
}
