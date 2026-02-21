// Downloader tab rendering.
// - Presents a 2-step flow for yt-dlp downloads.
//   Step 1: URL entry and metadata fetch.
//   Step 2: quality selection and download start.
// - Reuses the shared tool-output panel component for streamed process output.
// - Keeps layout/focus behavior consistent with the editor tab so navigation stays predictable.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::{
    app::App,
    model::{DownloaderStep, Focus},
};

use super::super::{
    output_panel::{LogPanelStateView, render_log_panel},
    pane_border_style,
};

const INPUT_LABEL_COL_WIDTH: usize = 12;
const MAX_QUALITY_ROWS: usize = 8;

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
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border_style(form_focused, Color::LightYellow))
        .title("Downloader");
    let inner = panel.inner(area);
    frame.render_widget(panel, area);

    if inner.width < 4 || inner.height < 3 {
        return;
    }

    if !app.downloader_available() {
        let warning_panel = Paragraph::new(vec![
            warning("WARNING: Downloader requires yt-dlp in PATH."),
            warning("Install yt-dlp, then restart this app."),
        ])
        .alignment(Alignment::Left);
        frame.render_widget(warning_panel, inner);
        return;
    }

    match app.downloader_step() {
        DownloaderStep::UrlInput => render_url_step(frame, app, form_focused, inner),
        DownloaderStep::QualitySelect => render_quality_step(frame, app, form_focused, inner),
    }
}

fn render_downloader_output(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let title = "TOOL OUTPUT";
    let visible_line_count = area.height.saturating_sub(2).max(1) as usize;

    render_log_panel(
        frame,
        area,
        LogPanelStateView {
            title,
            lines: app.downloader_output_lines(),
            scroll: app.clamped_downloader_output_scroll(visible_line_count),
            focused: focus == Focus::RightBottom,
            accent_color: Color::LightBlue,
            trim_wrapped_lines: false,
            title_hint_right: Some("(press x to cancel)"),
        },
    );
}

fn render_url_step(frame: &mut Frame, app: &App, form_focused: bool, area: Rect) {
    let url_cursor =
        (form_focused && app.downloader_accepts_text_input()).then_some(app.downloader_url_cursor);

    let step_line = if app.downloader_is_fetching_qualities() {
        format!(
            "Step 1/2: Fetching video qualities {}",
            spinner_glyph(app.downloader_spinner_frame)
        )
    } else {
        "Step 1/2: Enter URL".to_string()
    };

    let lines = vec![
        Line::styled(
            step_line,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        input_line("URL", &app.downloader_url, url_cursor),
        Line::from(""),
        Line::styled(
            "Enter: fetch video qualities",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let panel = Paragraph::new(lines).alignment(Alignment::Left);
    frame.render_widget(panel, area);
}

fn render_quality_step(frame: &mut Frame, app: &App, form_focused: bool, area: Rect) {
    let show_playlist_option = app.downloader_playlist_available();
    let header_height = if show_playlist_option { 8 } else { 7 };
    let [header_area, list_region] =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).areas(area);
    let (selected, total) = app.downloader_quality_position();
    let selector = app.downloader_selected_quality_selector();
    let pick_row = if app.downloader_audio_only_enabled() {
        format!("audio-only  ({selector})")
    } else {
        format!("{selected}/{total}  ({selector})")
    };
    let option_focus = app.downloader_option_focus_index();
    let list_focused = app.downloader_quality_list_focused();
    let title_or_url = app.downloader_video_title().unwrap_or(app.downloader_url.trim());

    let mut header_lines = vec![
        Line::styled(
            "Step 2/2: Select video quality",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "Backspace: return to URL input",
            Style::default().fg(Color::DarkGray),
        ),
        row(
            "Title",
            truncate_middle(
                title_or_url,
                area.width.saturating_sub(14) as usize,
            ),
        ),
        row("Pick", pick_row),
        checkbox_line(
            "Audio only",
            app.downloader_audio_only_enabled(),
            form_focused && option_focus == Some(0),
        ),
        checkbox_line(
            "Sponsorblock",
            app.downloader_sponsorblock_enabled(),
            form_focused && option_focus == Some(1),
        ),
        checkbox_line(
            "Subtitles",
            app.downloader_subtitles_enabled(),
            form_focused && option_focus == Some(2),
        ),
    ];
    if show_playlist_option {
        header_lines.push(checkbox_line_with_hint(
            "Playlist",
            app.downloader_playlist_enabled(),
            form_focused && option_focus == Some(3),
            "downloads the whole playlist",
        ));
    }
    frame.render_widget(Paragraph::new(header_lines), header_area);

    if list_region.height < 4 {
        return;
    }

    let max_rows = (list_region.height.saturating_sub(3) as usize)
        .max(1)
        .min(MAX_QUALITY_ROWS);
    let list_height = (max_rows as u16 + 3).min(list_region.height);
    let [list_area, _] =
        Layout::vertical([Constraint::Length(list_height), Constraint::Min(0)]).areas(list_region);

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title("QUALITY")
        .border_style(pane_border_style(
            form_focused && list_focused,
            Color::LightYellow,
        ));
    let inner = list_block.inner(list_area);
    frame.render_widget(list_block, list_area);

    if inner.height < 2 || inner.width < 8 {
        return;
    }

    let [columns_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(
        Paragraph::new(Line::styled(
            app.downloader_quality_header_row(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        columns_area,
    );

    let visible_rows = (rows_area.height as usize).max(1).min(MAX_QUALITY_ROWS);
    let (rows, selected_in_view) = app.downloader_visible_quality_rows(visible_rows);
    let items = rows
        .iter()
        .map(|row_text| ListItem::new(row_text.clone()))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(selected_in_view.min(items.len().saturating_sub(1))));
    }

    let list = List::new(items).highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, rows_area, &mut state);
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
    const LABEL_COL_WIDTH: usize = INPUT_LABEL_COL_WIDTH;
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

fn checkbox_line(label: &str, checked: bool, focused: bool) -> Line<'static> {
    const LABEL_COL_WIDTH: usize = INPUT_LABEL_COL_WIDTH;
    let label_cell = format!("{label:<LABEL_COL_WIDTH$}");
    let box_text = if checked { "[x]" } else { "[ ]" };
    let (label_style, value_style) = if focused {
        (
            Style::default()
                .fg(Color::Black)
                .bg(Color::Gray)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::White),
        )
    };

    Line::from(vec![
        Span::styled(label_cell, label_style),
        Span::raw("  "),
        Span::styled(box_text.to_string(), value_style),
    ])
}

fn checkbox_line_with_hint(label: &str, checked: bool, focused: bool, hint: &str) -> Line<'static> {
    let mut line = checkbox_line(label, checked, focused);
    line.spans.push(Span::raw("  "));
    line.spans.push(Span::styled(
        hint.to_string(),
        Style::default().fg(Color::DarkGray),
    ));
    line
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

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let left = keep / 2;
    let right = keep - left;
    let start = chars[..left].iter().collect::<String>();
    let end = chars[chars.len() - right..].iter().collect::<String>();
    format!("{start}...{end}")
}

fn spinner_glyph(frame: usize) -> char {
    const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
    FRAMES[frame % FRAMES.len()]
}
