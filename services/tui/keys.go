package tui

import "github.com/charmbracelet/bubbles/key"

type keyMap struct {
	Up     key.Binding
	Down   key.Binding
	Enter  key.Binding
	Esc    key.Binding
	Start  key.Binding
	New    key.Binding
	Delete key.Binding
	Rename key.Binding
	Snap   key.Binding
	Account key.Binding
	Help   key.Binding
	Quit   key.Binding
}

var keys = keyMap{
	Up:     key.NewBinding(key.WithKeys("up", "k"), key.WithHelp("↑/k", "up")),
	Down:   key.NewBinding(key.WithKeys("down", "j"), key.WithHelp("↓/j", "down")),
	Enter:  key.NewBinding(key.WithKeys("enter"), key.WithHelp("enter", "select")),
	Esc:    key.NewBinding(key.WithKeys("esc"), key.WithHelp("esc", "back")),
	Start:  key.NewBinding(key.WithKeys("s"), key.WithHelp("s", "start/stop")),
	New:    key.NewBinding(key.WithKeys("n"), key.WithHelp("n", "new VM")),
	Delete: key.NewBinding(key.WithKeys("d"), key.WithHelp("d", "delete")),
	Rename: key.NewBinding(key.WithKeys("r"), key.WithHelp("r", "rename")),
	Snap:   key.NewBinding(key.WithKeys("p"), key.WithHelp("p", "snapshot")),
	Account: key.NewBinding(key.WithKeys("a"), key.WithHelp("a", "account")),
	Help:   key.NewBinding(key.WithKeys("?"), key.WithHelp("?", "help")),
	Quit:   key.NewBinding(key.WithKeys("q", "ctrl+c"), key.WithHelp("q", "quit")),
}
