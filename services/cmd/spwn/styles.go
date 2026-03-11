package main

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/lipgloss"
	ltable "github.com/charmbracelet/lipgloss/table"
)

// Catppuccin Mocha palette — mirrors tui/styles.go.
var (
	colorGreen  = lipgloss.Color("#a6e3a1")
	colorYellow = lipgloss.Color("#f9e2af")
	colorRed    = lipgloss.Color("#f38ba8")
	colorBlue   = lipgloss.Color("#89b4fa")
	colorDim    = lipgloss.Color("#585b70")
	colorText   = lipgloss.Color("#cdd6f4")
	colorSubtle = lipgloss.Color("#6c7086")

	styleHeader = lipgloss.NewStyle().Bold(true).Foreground(colorText)
	styleDim    = lipgloss.NewStyle().Foreground(colorDim)
	styleKey    = lipgloss.NewStyle().Foreground(colorSubtle).Width(10)
	styleVal    = lipgloss.NewStyle().Foreground(colorText)
	styleOK     = lipgloss.NewStyle().Foreground(colorGreen)
	styleHint   = lipgloss.NewStyle().Foreground(colorDim).Italic(true)

	stylePanelBorder = lipgloss.NewStyle().
				Border(lipgloss.RoundedBorder()).
				BorderForeground(colorSubtle).
				Padding(0, 1)
)

func statusStyle(status string) lipgloss.Style {
	switch status {
	case "running":
		return lipgloss.NewStyle().Foreground(colorGreen)
	case "starting", "stopping", "snapshotting":
		return lipgloss.NewStyle().Foreground(colorYellow)
	case "error":
		return lipgloss.NewStyle().Foreground(colorRed)
	default:
		return lipgloss.NewStyle().Foreground(colorSubtle)
	}
}

func statusSymbol(status string) string {
	switch status {
	case "running":
		return "●"
	case "starting", "stopping", "snapshotting":
		return "◐"
	case "error":
		return "✗"
	default:
		return "○"
	}
}

func statusDot(status string) string {
	return statusStyle(status).Render(statusSymbol(status))
}

func statusBadge(status string) string {
	return statusStyle(status).Render(status)
}

// kvLine returns a formatted "key  value" line for use inside panels.
func kvLine(key, val string) string {
	return styleKey.Render(key) + "  " + styleVal.Render(val)
}

func printOK(msg string) {
	fmt.Println(styleOK.Render("✓") + "  " + msg)
}

func printHint(msg string) {
	fmt.Println(styleHint.Render(msg))
}

// newTable returns a styled lipgloss table with no outer border,
// a dim header separator, and a StyleFunc for per-cell coloring.
func newTable(styleFunc ltable.StyleFunc) *ltable.Table {
	return ltable.New().
		BorderTop(false).
		BorderBottom(false).
		BorderLeft(false).
		BorderRight(false).
		BorderColumn(false).
		BorderHeader(true).
		BorderStyle(lipgloss.NewStyle().Foreground(colorDim)).
		StyleFunc(func(row, col int) lipgloss.Style {
			return styleFunc(row, col).PaddingRight(3)
		})
}

// panel renders content inside a rounded border box with a title in the top border.
func panel(title, content string) string {
	inner := stylePanelBorder.Render(content)
	if title == "" {
		return inner
	}
	// Inject the title into the top border line.
	lines := strings.SplitN(inner, "\n", 2)
	if len(lines) < 2 {
		return inner
	}
	titleStr := " " + lipgloss.NewStyle().Bold(true).Foreground(colorBlue).Render(title) + " "
	top := lines[0]
	if len(top) > 3 {
		top = "╭─" + titleStr + strings.Repeat("─", max(0, lipgloss.Width(top)-lipgloss.Width(titleStr)-3)) + "╮"
	}
	return top + "\n" + lines[1]
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
