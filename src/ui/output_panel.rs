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
    const OVERSCAN_MULTIPLIER: usize = 4;
    const MIN_WINDOW_LINES: usize = 64;
    const FOCUS_HINT: &str = "(ctrl+o)";

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(log_panel_border_style(panel.focused, panel.accent_color))
        .title_top(Line::from(panel.title).left_aligned())
        .title_top(Line::from(FOCUS_HINT).right_aligned());
    if let Some(hint) = panel.title_hint_right {
        block = block.title_bottom(Line::from(hint).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let visible_line_count = inner.height.max(1) as usize;
    let max_scroll_top = panel.lines.len().saturating_sub(visible_line_count);
    let scroll_top = panel.scroll.min(max_scroll_top);

    // Render only a window near the current viewport to avoid O(total_lines)
    // allocation every frame when tools stream large outputs.
    let window_len = (visible_line_count * OVERSCAN_MULTIPLIER).max(MIN_WINDOW_LINES);
    let half_window = window_len / 2;
    let mut window_start = scroll_top.saturating_sub(half_window);
    let mut window_end = (window_start + window_len).min(panel.lines.len());
    if window_end.saturating_sub(window_start) < window_len {
        window_start = window_end.saturating_sub(window_len);
        window_end = (window_start + window_len).min(panel.lines.len());
    }

    let lines = panel.lines[window_start..window_end]
        .iter()
        .map(String::as_str)
        .map(Line::from)
        .collect::<Vec<_>>();
    let relative_scroll = scroll_top.saturating_sub(window_start);

    let widget = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap {
            trim: panel.trim_wrapped_lines,
        })
        .scroll((relative_scroll.min(u16::MAX as usize) as u16, 0));

    frame.render_widget(widget, inner);
}

fn log_panel_border_style(is_focused: bool, accent: Color) -> Style {
    if is_focused {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
