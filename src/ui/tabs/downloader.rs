// Downloader tab rendering.
// - Shows the downloader form (URL + target directory) in the top pane.
// - Reuses the shared log panel component for streamed process output.
// - Keeps layout/focus behavior consistent with the editor tab so navigation stays predictable.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{app::App, model::Focus};

use super::super::{
    output_panel::{LogPanelStateView, render_log_panel},
    pane_border_style,
};

const INPUT_LABEL_COL_WIDTH: usize = 11;

pub fn render_downloader_tab(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let right_constraints = if focus == Focus::RightBottom {
        [Constraint::Percentage(30), Constraint::Percentage(70)]
    } else {
        [Constraint::Min(0), Constraint::Length(8)]
    };
    let [top, bottom] = Layout::vertical(right_constraints).areas(area);

    render_downloader_form(frame, app, focus, top);
    render_downloader_output(frame, app, focus, bottom);
}

fn render_downloader_form(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let form_focused = focus == Focus::RightTop;
    let cursor = form_focused.then_some(app.downloader_url_cursor);

    let mut lines = vec![
        section("DOWNLOAD SETUP"),
        Line::from(""),
        row("Tool", "yt-dlp".to_string()),
        row("Directory", app.cwd.display().to_string()),
        Line::from(""),
        input_line("URL", &app.downloader_url, cursor),
        Line::from(""),
        Line::from("Press Enter to start the download."),
        Line::from("Downloaded files are saved into the current browser directory."),
    ];

    if !app.downloader_available() {
        lines.insert(
            2,
            warning("WARNING: Downloader requires yt-dlp in PATH. Downloads are disabled."),
        );
        lines.insert(3, warning("Install yt-dlp, then restart this app."));
        lines.insert(4, Line::from(""));
    }

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(pane_border_style(form_focused, Color::LightYellow))
                .title("Downloader"),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    frame.render_widget(panel, area);
}

fn render_downloader_output(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let title = if app.downloader_is_running() {
        format!(
            "downloader output {} running (scroll: {})",
            app.downloader_spinner_glyph(),
            app.downloader_output_scroll()
        )
    } else {
        format!("downloader output (scroll: {})", app.downloader_output_scroll())
    };

    render_log_panel(
        frame,
        area,
        LogPanelStateView {
            title: &title,
            lines: app.downloader_output_lines(),
            scroll: app.downloader_output_scroll(),
            focused: focus == Focus::RightBottom,
            accent_color: Color::LightBlue,
            trim_wrapped_lines: false,
        },
    );
}

fn section(title: &str) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn warning(message: &str) -> Line<'static> {
    Line::styled(
        message.to_string(),
        Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
    )
}

fn row(label: &str, value: String) -> Line<'static> {
    const LABEL_COL_WIDTH: usize = 10;
    let label_cell = format!("{label:<LABEL_COL_WIDTH$}");
    Line::from(vec![
        Span::styled(
            label_cell,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(value),
    ])
}

fn input_line(label: &str, value: &str, active_cursor: Option<usize>) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");
    let active = active_cursor.is_some();

    let mut spans = vec![
        Span::styled(
            label_cell,
            if active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Gray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD)
            },
        ),
        Span::raw("  "),
    ];

    let chars = value.chars().collect::<Vec<_>>();
    let cursor = active_cursor.unwrap_or(0).min(chars.len());
    let value_style = if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let cursor_style = Style::default()
        .fg(Color::Black)
        .bg(Color::White)
        .add_modifier(Modifier::BOLD);

    for (index, ch) in chars.iter().enumerate() {
        let style = if active && index == cursor {
            cursor_style
        } else {
            value_style
        };
        spans.push(Span::styled(ch.to_string(), style));
    }

    if active && cursor == chars.len() {
        spans.push(Span::styled(" ".to_string(), cursor_style));
    }

    Line::from(spans)
}
