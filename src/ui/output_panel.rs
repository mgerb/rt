// Reusable scrollable text-output panel.
// - Renders title, content lines, and scroll offset in a consistent style.
// - Applies focus-aware border styling so any tab can reuse it for logs/output.
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct LogPanelStateView<'a> {
    pub title: &'a str,
    pub lines: &'a [String],
    pub scroll: usize,
    pub focused: bool,
    pub accent_color: Color,
    pub trim_wrapped_lines: bool,
}

pub fn render_log_panel(frame: &mut Frame, area: Rect, panel: LogPanelStateView<'_>) {
    let lines = panel
        .lines
        .iter()
        .map(String::as_str)
        .map(Line::from)
        .collect::<Vec<_>>();

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(log_panel_border_style(panel.focused, panel.accent_color))
                .title(panel.title),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap {
            trim: panel.trim_wrapped_lines,
        })
        .scroll((panel.scroll.min(u16::MAX as usize) as u16, 0));

    frame.render_widget(widget, area);
}

fn log_panel_border_style(is_focused: bool, accent: Color) -> Style {
    if is_focused {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
