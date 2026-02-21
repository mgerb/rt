// Editor-tab rendering.
// - Formats selected-video metadata and editable editor/output fields.
// - Highlights active inputs/focus state for keyboard-driven editing.
// - Renders the ffmpeg output panel beneath the form.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    app::App,
    media::scaled_resolution_for_percent,
    model::{Focus, InputField, TimeInput},
};

use super::super::{
    output_panel::{LogPanelStateView, render_log_panel},
    pane_border_style,
};

const INPUT_LABEL_COL_WIDTH: usize = 11;

pub fn render_editor_tab(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let right_constraints = if focus == Focus::RightBottom {
        [Constraint::Percentage(30), Constraint::Percentage(70)]
    } else {
        [Constraint::Min(0), Constraint::Length(8)]
    };
    let [top, bottom] = Layout::vertical(right_constraints).areas(area);

    render_editor_pane(frame, app, focus, top);
    render_ffmpeg_output_pane(frame, app, focus, bottom);
}

fn render_editor_pane(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let mut lines = Vec::new();
    if !app.ffmpeg_available() {
        lines.push(ffmpeg_warning_line(
            "WARNING: ffmpeg not found in PATH. Editor export is disabled.",
        ));
        lines.push(ffmpeg_warning_line(
            "Install ffmpeg, then restart this app.",
        ));
        lines.push(Line::from(""));
    }

    if let Some(video) = &app.selected_video {
        let start_active_part = (focus == Focus::RightTop && app.active_input == InputField::Start)
            .then_some(app.start_part);
        let end_active_part = (focus == Focus::RightTop && app.active_input == InputField::End)
            .then_some(app.end_part);
        let format_active = focus == Focus::RightTop && app.active_input == InputField::Format;
        let fps_active_cursor = (focus == Focus::RightTop && app.active_input == InputField::Fps)
            .then_some(app.output_fps_cursor);
        let bitrate_active_cursor = (app.bitrate_enabled()
            && focus == Focus::RightTop
            && app.active_input == InputField::Bitrate)
            .then_some(app.output_bitrate_cursor);
        let scale_percent_active_cursor = (focus == Focus::RightTop
            && app.active_input == InputField::ScalePercent)
            .then_some(app.output_scale_percent_cursor);
        let remove_audio_active =
            focus == Focus::RightTop && app.active_input == InputField::RemoveAudio;
        let output_active_cursor = (focus == Focus::RightTop
            && app.active_input == InputField::Output)
            .then_some(app.output_cursor);

        let filename = video
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| video.display().to_string());
        lines.push(editor_row("Video", filename));
        lines.push(editor_row("Path", video.display().to_string()));
        lines.push(Line::from(""));
        lines.push(editor_section("VIDEO STATS"));
        lines.push(Line::from(""));

        if let Some(stats) = &app.selected_video_stats {
            lines.push(editor_row("Duration", stats.duration.clone()));
            lines.push(editor_row("Resolution", stats.resolution.clone()));
            lines.push(editor_row("FPS", stats.fps.clone()));
            lines.push(editor_row("Video", stats.video_codec.clone()));
            lines.push(editor_row("Audio", stats.audio_codec.clone()));
            lines.push(editor_row("Size", stats.size.clone()));
            lines.push(editor_row("Bitrate", stats.bitrate.clone()));
        } else {
            lines.push(editor_row("Stats", "unavailable".to_string()));
        }

        lines.push(Line::from(""));
        lines.push(editor_separator());
        lines.push(Line::from(""));
        lines.push(editor_section("TIME RANGE"));
        lines.push(Line::from(""));
        lines.push(input_hint_line("", "HH:MM:SS"));
        lines.push(time_input_line(
            "Start time",
            &app.start_time,
            start_active_part,
        ));
        lines.push(time_input_line("End time", &app.end_time, end_active_part));
        lines.push(Line::from(""));
        lines.push(editor_section("OUTPUT"));
        lines.push(Line::from(""));
        lines.push(choice_input_line(
            "Format",
            app.output_format,
            format_active,
        ));
        if app.video_options_enabled() {
            lines.push(input_line("FPS", &app.output_fps, fps_active_cursor));
            if app.bitrate_enabled() {
                lines.push(input_line(
                    "Bitrate",
                    &app.output_bitrate_kbps,
                    bitrate_active_cursor,
                ));
            } else {
                lines.push(disabled_input_line("Bitrate", "n/a for GIF"));
            }
            lines.push(input_line_with_suffix(
                "Scale %",
                &app.output_scale_percent,
                scale_percent_active_cursor,
                &preview_scaled_resolution(app),
            ));
            lines.push(checkbox_input_line(
                "Remove audio",
                app.remove_audio,
                remove_audio_active,
            ));
        } else {
            lines.push(disabled_input_line("FPS", "n/a for audio-only"));
            lines.push(disabled_input_line("Bitrate", "n/a for audio-only"));
            lines.push(disabled_input_line("Scale %", "n/a for audio-only"));
            lines.push(disabled_input_line("Remove audio", "n/a for audio-only"));
        }
        lines.push(input_line("Output", &app.output_name, output_active_cursor));
        lines.push(Line::from(""));
    } else {
        lines.push(editor_section("NO VIDEO SELECTED"));
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
                .title("Editor"),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

    frame.render_widget(details, area);
}

fn render_ffmpeg_output_pane(frame: &mut Frame, app: &App, focus: Focus, area: Rect) {
    let title = "TOOL OUTPUT";

    render_log_panel(
        frame,
        area,
        LogPanelStateView {
            title,
            lines: app.ffmpeg_output_lines(),
            scroll: app.ffmpeg_output_scroll(),
            focused: focus == Focus::RightBottom,
            accent_color: Color::LightMagenta,
            trim_wrapped_lines: false,
        },
    );
}

fn editor_section(title: &str) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn ffmpeg_warning_line(message: &str) -> Line<'static> {
    Line::styled(
        message.to_string(),
        Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
    )
}

fn editor_separator() -> Line<'static> {
    Line::styled(
        "------------------------------------------------".to_string(),
        Style::default().fg(Color::DarkGray),
    )
}

fn editor_row(label: &str, value: String) -> Line<'static> {
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

fn input_line_with_suffix(
    label: &str,
    value: &str,
    active_cursor: Option<usize>,
    suffix: &str,
) -> Line<'static> {
    let mut line = input_line(label, value, active_cursor);
    if !suffix.is_empty() {
        line.spans.push(Span::raw("  "));
        line.spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    line
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

fn disabled_input_line(label: &str, value: &str) -> Line<'static> {
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

fn preview_scaled_resolution(app: &App) -> String {
    let Some(stats) = app.selected_video_stats.as_ref() else {
        return "n/a".to_string();
    };
    let (Some(width), Some(height)) = (stats.width, stats.height) else {
        return "n/a".to_string();
    };
    let Some(percent) = parse_scale_percent_for_preview(&app.output_scale_percent) else {
        return "invalid (1-100%)".to_string();
    };
    let (scaled_width, scaled_height) = scaled_resolution_for_percent(width, height, percent);
    format!("{scaled_width}x{scaled_height} ({percent}%)")
}

fn parse_scale_percent_for_preview(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(100);
    }
    trimmed
        .parse::<u32>()
        .ok()
        .filter(|value| *value >= 1 && *value <= 100)
}
