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
    pub title_hint_right: Option<&'a str>,
}

pub fn render_log_panel(frame: &mut Frame, area: Rect, panel: LogPanelStateView<'_>) {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(log_panel_border_style(panel.focused, panel.accent_color))
        .title_top(Line::from(panel.title).left_aligned());
    if let Some(hint) = panel.title_hint_right {
        block = block.title_top(Line::from(hint).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = panel
        .lines
        .iter()
        .map(String::as_str)
        .map(Line::from)
        .collect::<Vec<_>>();
    let visible_line_count = inner.height.max(1) as usize;
    let max_scroll_top = lines.len().saturating_sub(visible_line_count);
    let scroll_top = panel.scroll.min(max_scroll_top);

    let widget = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap {
            trim: panel.trim_wrapped_lines,
        })
        .scroll((scroll_top.min(u16::MAX as usize) as u16, 0));

    frame.render_widget(widget, inner);
}

fn log_panel_border_style(is_focused: bool, accent: Color) -> Style {
    if is_focused {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
