package tui

import "github.com/charmbracelet/lipgloss"

var (
	colorRunning  = lipgloss.Color("#a6e3a1") // green
	colorStopped  = lipgloss.Color("#6c7086") // dim
	colorStarting = lipgloss.Color("#f9e2af") // yellow
	colorError    = lipgloss.Color("#f38ba8") // red

	styleHeader = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#cdd6f4"))

	styleSelected = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#89b4fa"))

	styleDim = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#585b70"))

	styleError = lipgloss.NewStyle().
			Foreground(colorError)

	styleSuccess = lipgloss.NewStyle().
			Foreground(colorRunning)

	styleBorder = lipgloss.NewStyle().
			Border(lipgloss.NormalBorder()).
			BorderForeground(lipgloss.Color("#313244"))

	styleStatusBar = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#585b70"))

	styleTitle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#cdd6f4"))
)

func statusColor(status string) lipgloss.Style {
	switch status {
	case "running":
		return lipgloss.NewStyle().Foreground(colorRunning)
	case "starting", "stopping":
		return lipgloss.NewStyle().Foreground(colorStarting)
	case "error":
		return lipgloss.NewStyle().Foreground(colorError)
	default:
		return lipgloss.NewStyle().Foreground(colorStopped)
	}
}

func statusDot(status string) string {
	switch status {
	case "running":
		return statusColor(status).Render("●")
	case "starting", "stopping":
		return statusColor(status).Render("◐")
	case "error":
		return statusColor(status).Render("✗")
	default:
		return statusColor(status).Render("○")
	}
}
