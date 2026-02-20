use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::App,
    media::is_video_file,
    model::{Focus, InputField, TimeInput, TimeSection},
};

pub fn render(frame: &mut Frame, app: &App, focus: Focus) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)])
            .areas(frame.area());
    let [right_top, right_bottom] =
        Layout::vertical([Constraint::Min(12), Constraint::Length(10)]).areas(right);

    render_files_pane(frame, app, focus, left);
    render_trim_pane(frame, app, focus, right_top);
    render_ffmpeg_output_pane(frame, app, focus, right_bottom);
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
            ListItem::new(format!("{prefix}{}", entry.name))
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
                .border_style(if focus == Focus::Left {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
                .title(format!("Files: {}", app.cwd.display())),
        )
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(files, area, &mut list_state);
}

fn render_trim_pane(frame: &mut Frame, app: &App, focus: Focus, area: ratatui::layout::Rect) {
    let mut lines = Vec::new();
    lines.push(Line::from("ffmpeg clip trimmer"));
    lines.push(Line::from(""));

    if let Some(video) = &app.selected_video {
        let start_active = if focus == Focus::RightTop {
            match app.active_input {
                InputField::Start(section) => Some(section),
                _ => None,
            }
        } else {
            None
        };

        let end_active = if focus == Focus::RightTop {
            match app.active_input {
                InputField::End(section) => Some(section),
                _ => None,
            }
        } else {
            None
        };

        let output_active = focus == Focus::RightTop && app.active_input == InputField::Output;

        lines.push(Line::from(format!("Input file: {}", video.display())));
        lines.push(time_input_line("Start time", &app.start_time, start_active));
        lines.push(time_input_line("End time", &app.end_time, end_active));
        lines.push(input_line("Output", &app.output_name, output_active));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Tab/Shift+Tab: move between time sections and output",
        ));
        lines.push(Line::from("Type digits for time sections"));
        lines.push(Line::from("Enter: run ffmpeg trim"));
        lines.push(Line::from("Ctrl+h / Ctrl+l: switch columns"));
        lines.push(Line::from("Ctrl+j / Ctrl+k: move between windows"));
        lines.push(Line::from("ffmpeg output pane: j/k or Up/Down to scroll"));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(
            "Select a video in the left pane and press Enter.",
        ));
        lines.push(Line::from(
            "Supported: mp4, mov, mkv, avi, webm, m4v, mpeg, mpg, wmv, flv",
        ));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(format!("Status: {}", app.status_message)));

    let details = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if focus == Focus::RightTop {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
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

    let ffmpeg_log = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if focus == Focus::RightBottom {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
                .title(format!("ffmpeg output (scroll: {})", app.ffmpeg_scroll)),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .scroll((app.ffmpeg_scroll.min(u16::MAX as usize) as u16, 0));

    frame.render_widget(ffmpeg_log, area);
}

fn input_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let input_style = if active {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    Line::from(vec![
        Span::raw(format!("{label}: ")),
        Span::styled(value.to_string(), input_style),
    ])
}

fn time_input_line(label: &str, value: &TimeInput, active: Option<TimeSection>) -> Line<'static> {
    let style_for = |section: TimeSection| {
        if active == Some(section) {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        }
    };

    Line::from(vec![
        Span::raw(format!("{label}: ")),
        Span::styled(
            value.part(TimeSection::Hours).to_string(),
            style_for(TimeSection::Hours),
        ),
        Span::raw(":"),
        Span::styled(
            value.part(TimeSection::Minutes).to_string(),
            style_for(TimeSection::Minutes),
        ),
        Span::raw(":"),
        Span::styled(
            value.part(TimeSection::Seconds).to_string(),
            style_for(TimeSection::Seconds),
        ),
    ])
}
