use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Color,
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::model::Focus;

use super::super::pane_border_style;

pub fn render_hello_tab(frame: &mut Frame, focus: Focus, area: Rect) {
    let focused = focus == Focus::RightTop || focus == Focus::RightBottom;
    let lines = vec![
        Line::from("HELLO"),
        Line::from(""),
        Line::from("This is a placeholder tab."),
        Line::from("Press Shift+H / Shift+L or 1 / 2 to switch tabs."),
    ];

    let hello = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Hello")
                .border_style(pane_border_style(focused, Color::LightYellow)),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    frame.render_widget(hello, area);
}
