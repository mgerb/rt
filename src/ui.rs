use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::App,
    media::is_video_file,
    model::{Focus, InputField, TimeInput},
};

const INPUT_LABEL_COL_WIDTH: usize = 11;

pub fn render(frame: &mut Frame, app: &App, focus: Focus) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)]).areas(content);
    let right_constraints = if focus == Focus::RightBottom {
        [Constraint::Percentage(30), Constraint::Percentage(70)]
    } else {
        [Constraint::Min(0), Constraint::Length(8)]
    };
    let [right_top, right_bottom] = Layout::vertical(right_constraints).areas(right);

    render_files_pane(frame, app, focus, left);
    render_trim_pane(frame, app, focus, right_top);
    render_ffmpeg_output_pane(frame, app, focus, right_bottom);
    render_footer_hint(frame, footer);
    if app.show_keybinds {
        render_keybinds_popup(frame);
    }
}

fn render_files_pane(frame: &mut Frame, app: &App, focus: Focus, area: ratatui::layout::Rect) {
    let file_items = app
        .entries
        .iter()
        .map(|entry| {
            let prefix = if entry.is_dir {
                "[D] "
            } else if is_video_file(&entry.path) {
                "[V] "
            } else {
                "[F] "
            };
            let line = format!("{prefix}{}", entry.name);
            if is_video_file(&entry.path) {
                ListItem::new(Line::styled(line, Style::default().fg(Color::LightGreen)))
            } else {
                ListItem::new(line)
            }
        })
        .collect::<Vec<_>>();

    let mut list_state = ListState::default();
    if !app.entries.is_empty() {
        list_state.select(Some(app.selected));
    }

    let files = List::new(file_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(pane_border_style(focus == Focus::Left, Color::LightBlue))
                .title(format!("Files: {}", app.cwd.display())),
        )
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(files, area, &mut list_state);
}

fn render_trim_pane(frame: &mut Frame, app: &App, focus: Focus, area: ratatui::layout::Rect) {
    let mut lines = Vec::new();
    lines.push(trim_section("CLIP SETUP"));
    lines.push(Line::from(""));

    if let Some(video) = &app.selected_video {
        let start_active_part = (focus == Focus::RightTop && app.active_input == InputField::Start)
            .then_some(app.start_part);
        let end_active_part = (focus == Focus::RightTop && app.active_input == InputField::End)
            .then_some(app.end_part);
        let format_active = focus == Focus::RightTop && app.active_input == InputField::Format;
        let fps_active_cursor = (focus == Focus::RightTop && app.active_input == InputField::Fps)
            .then_some(app.output_fps_cursor);
        let bitrate_active_cursor = (focus == Focus::RightTop
            && app.active_input == InputField::Bitrate)
            .then_some(app.output_bitrate_cursor);
        let remove_audio_active =
            focus == Focus::RightTop && app.active_input == InputField::RemoveAudio;
        let output_active_cursor = (focus == Focus::RightTop
            && app.active_input == InputField::Output)
            .then_some(app.output_cursor);

        let filename = video
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| video.display().to_string());
        lines.push(trim_row("Video", filename));
        lines.push(trim_row("Path", video.display().to_string()));
        lines.push(Line::from(""));
        lines.push(trim_section("VIDEO STATS"));
        lines.push(Line::from(""));
        if let Some(stats) = &app.selected_video_stats {
            lines.push(trim_row("Duration", stats.duration.clone()));
            lines.push(trim_row("Resolution", stats.resolution.clone()));
            lines.push(trim_row("FPS", stats.fps.clone()));
            lines.push(trim_row("Video", stats.video_codec.clone()));
            lines.push(trim_row("Audio", stats.audio_codec.clone()));
            lines.push(trim_row("Size", stats.size.clone()));
            lines.push(trim_row("Bitrate", stats.bitrate.clone()));
        } else {
            lines.push(trim_row("Stats", "unavailable".to_string()));
        }
        lines.push(Line::from(""));
        lines.push(trim_separator());
        lines.push(Line::from(""));
        lines.push(trim_section("TIME RANGE"));
        lines.push(Line::from(""));
        lines.push(input_hint_line("Format", "HH:MM:SS"));
        lines.push(Line::from(""));
        lines.push(time_input_line(
            "Start time",
            &app.start_time,
            start_active_part,
        ));
        lines.push(time_input_line("End time", &app.end_time, end_active_part));
        lines.push(Line::from(""));
        lines.push(trim_section("OUTPUT"));
        lines.push(Line::from(""));
        lines.push(choice_input_line(
            "Format",
            app.output_format,
            format_active,
        ));
        lines.push(input_line("FPS", &app.output_fps, fps_active_cursor));
        lines.push(input_line(
            "Bitrate",
            &app.output_bitrate_kbps,
            bitrate_active_cursor,
        ));
        lines.push(checkbox_input_line(
            "Remove audio",
            app.remove_audio,
            remove_audio_active,
        ));
        lines.push(input_line("Output", &app.output_name, output_active_cursor));
        lines.push(Line::from(""));
    } else {
        lines.push(trim_section("NO VIDEO SELECTED"));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Select a video in the left pane and press Enter.",
        ));
        lines.push(Line::from(
            "Supported: mp4, mov, mkv, avi, webm, m4v, mpeg, mpg, wmv, flv",
        ));
        lines.push(Line::from(""));
    }

    let details = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(pane_border_style(
                    focus == Focus::RightTop,
                    Color::LightYellow,
                ))
                .title("Trim"),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    frame.render_widget(details, area);
}

fn render_ffmpeg_output_pane(
    frame: &mut Frame,
    app: &App,
    focus: Focus,
    area: ratatui::layout::Rect,
) {
    let log_lines = app
        .ffmpeg_output
        .iter()
        .cloned()
        .map(Line::from)
        .collect::<Vec<_>>();

    let title = if app.ffmpeg_is_running() {
        format!(
            "ffmpeg output {} running (scroll: {})",
            app.ffmpeg_spinner_glyph(),
            app.ffmpeg_scroll
        )
    } else {
        format!("ffmpeg output (scroll: {})", app.ffmpeg_scroll)
    };

    let ffmpeg_log = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(pane_border_style(
                    focus == Focus::RightBottom,
                    Color::LightMagenta,
                ))
                .title(title),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .scroll((app.ffmpeg_scroll.min(u16::MAX as usize) as u16, 0));

    frame.render_widget(ffmpeg_log, area);
}

fn trim_section(title: &str) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn trim_separator() -> Line<'static> {
    Line::styled(
        "------------------------------------------------".to_string(),
        Style::default().fg(Color::DarkGray),
    )
}

fn trim_row(label: &str, value: String) -> Line<'static> {
    const LABEL_COL_WIDTH: usize = 10;
    const VALUE_MAX_CHARS: usize = 64;
    let label_cell = format!("{label:<LABEL_COL_WIDTH$}");
    Line::from(vec![
        Span::styled(
            label_cell,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(truncate_tail(&value, VALUE_MAX_CHARS)),
    ])
}

fn render_keybinds_popup(frame: &mut Frame) {
    let outer = frame.area();
    let [vertical] = Layout::vertical([Constraint::Percentage(70)])
        .flex(ratatui::layout::Flex::Center)
        .areas(outer);
    let [popup] = Layout::horizontal([Constraint::Percentage(70)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from("Press ? or Esc to close this window."),
        Line::from(""),
        keybind_section("GLOBAL"),
        keybind_row("?", "toggle keybinds popup"),
        keybind_row("Esc", "close popup / quit app"),
        keybind_row("Ctrl+c", "quit app"),
        Line::from(""),
        keybind_section("WINDOW FOCUS"),
        keybind_row("Ctrl+h", "focus left browser"),
        keybind_row("Ctrl+l", "focus trim panel"),
        keybind_row("Ctrl+j / Ctrl+k", "move window focus"),
        keybind_row("Ctrl+o", "jump to output filename field"),
        Line::from(""),
        keybind_section("LEFT BROWSER"),
        keybind_row("j/k or Up/Down", "move selection"),
        keybind_row("Enter or l/Right", "open dir or select video"),
        keybind_row("h/Left/Backspace/-", "parent directory"),
        keybind_row("_", "initial directory"),
        keybind_row("r", "refresh listing"),
        Line::from(""),
        keybind_section("TRIM PANEL"),
        keybind_row("Tab / Shift+Tab", "move through time pieces and fields"),
        keybind_row("h/l", "cycle output format"),
        keybind_row("Left/Right", "move FPS/bitrate/output cursor"),
        keybind_row("Space", "toggle remove-audio checkbox"),
        keybind_row("Digits", "edit selected time piece, FPS, or bitrate"),
        keybind_row(
            "Backspace",
            "clear time piece / delete FPS/bitrate/output char",
        ),
        keybind_row("Enter", "run ffmpeg trim"),
        Line::from(""),
        keybind_section("FFMPEG OUTPUT"),
        keybind_row("j/k or Up/Down", "scroll output"),
    ];

    let popup_widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Keybinds"))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);

    frame.render_widget(popup_widget, popup);
}

fn keybind_section(title: &str) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn keybind_row(keys: &str, action: &str) -> Line<'static> {
    const KEY_COL_WIDTH: usize = 24;
    let keys_padded = format!("{keys:<KEY_COL_WIDTH$}");
    Line::from(vec![
        Span::styled(
            keys_padded,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(action.to_string()),
    ])
}

fn render_footer_hint(frame: &mut Frame, area: ratatui::layout::Rect) {
    let hint = Paragraph::new(Line::styled(
        "Press ? to see keyboard shortcuts",
        Style::default().fg(Color::DarkGray),
    ))
    .alignment(Alignment::Left);
    frame.render_widget(hint, area);
}

fn input_line(label: &str, value: &str, active_cursor: Option<usize>) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");
    let active = active_cursor.is_some();
    let value_style = input_value_style(active);
    let cursor_style = Style::default()
        .fg(Color::Black)
        .bg(Color::White)
        .add_modifier(Modifier::BOLD);

    let mut spans = vec![
        Span::styled(label_cell, input_label_style(active)),
        Span::raw("  "),
    ];

    let chars = value.chars().collect::<Vec<_>>();
    let cursor = active_cursor.unwrap_or(0).min(chars.len());

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

fn input_hint_line(label: &str, value: &str) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");
    Line::from(vec![
        Span::styled(
            label_cell,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(value.to_string(), Style::default().fg(Color::DarkGray)),
    ])
}

fn choice_input_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");

    Line::from(vec![
        Span::styled(label_cell, input_label_style(active)),
        Span::raw("  "),
        Span::styled(value.to_string(), input_value_style(active)),
    ])
}

fn checkbox_input_line(label: &str, checked: bool, active: bool) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");
    let mark = if checked { "[x]" } else { "[ ]" };

    Line::from(vec![
        Span::styled(label_cell, input_label_style(active)),
        Span::raw("  "),
        Span::styled(mark.to_string(), input_value_style(active)),
    ])
}

fn time_input_line(label: &str, value: &TimeInput, active_part: Option<usize>) -> Line<'static> {
    let label_cell = format!("{label:<INPUT_LABEL_COL_WIDTH$}");
    let mut spans = vec![
        Span::styled(label_cell, input_label_style(false)),
        Span::raw("  "),
    ];

    for part in 0..3 {
        spans.push(Span::styled(
            value.part(part).to_string(),
            time_part_style(active_part == Some(part)),
        ));
        if part < 2 {
            spans.push(Span::styled(":".to_string(), input_value_style(false)));
        }
    }

    Line::from(spans)
}

fn input_label_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD)
    }
}

fn input_value_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

fn time_part_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

fn pane_border_style(is_focused: bool, focused_color: Color) -> Style {
    if is_focused {
        Style::default()
            .fg(focused_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn truncate_tail(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }

    if max_chars == 1 {
        return "…".to_string();
    }

    let keep = max_chars - 1;
    let tail = value
        .chars()
        .skip(char_count.saturating_sub(keep))
        .collect::<String>();
    format!("…{tail}")
}
