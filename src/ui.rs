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
    model::{Focus, InputField, TimeInput, TimeSection},
};

pub fn render(frame: &mut Frame, app: &App, focus: Focus) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)]).areas(content);
    let [right_top, right_bottom] =
        Layout::vertical([Constraint::Min(12), Constraint::Length(10)]).areas(right);

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
        Line::from(""),
        keybind_section("LEFT BROWSER"),
        keybind_row("j/k or Up/Down", "move selection"),
        keybind_row("Enter or l/Right", "open dir or select video"),
        keybind_row("h/Left/Backspace/-", "parent directory"),
        keybind_row("_", "initial directory"),
        keybind_row("r", "refresh listing"),
        Line::from(""),
        keybind_section("TRIM PANEL"),
        keybind_row("Tab / Shift+Tab", "move input section"),
        keybind_row("Digits", "edit time fields"),
        keybind_row("Backspace", "edit active field"),
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
