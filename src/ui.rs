// Root UI composition and shared visual components.
// - Builds the global layout (left browser + right tab area + footer).
// - Renders shared chrome: tab bar, keybind popup, and delete-confirm modal.
// - Delegates tab-specific rendering to ui::tabs submodules.
mod output_panel;
mod tabs;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::{
    app::App,
    media::is_video_file,
    model::{Focus, RightTab},
};

pub fn render(frame: &mut Frame, app: &App, focus: Focus) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)]).areas(content);
    let [tabs_area, right_content] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(right);

    render_files_pane(frame, app, focus, left);
    render_right_tabs(frame, app, focus, tabs_area);

    match app.right_tab() {
        RightTab::Trim => tabs::trim::render_trim_tab(frame, app, focus, right_content),
        RightTab::Hello => tabs::hello::render_hello_tab(frame, focus, right_content),
    }

    render_footer_hint(frame, footer);
    if app.show_keybinds {
        render_keybinds_popup(frame);
    }
    if app.has_pending_delete() {
        render_delete_confirm_modal(frame, app);
    }
}

fn render_right_tabs(frame: &mut Frame, app: &App, focus: Focus, area: ratatui::layout::Rect) {
    let selected = RightTab::ALL
        .iter()
        .position(|tab| *tab == app.right_tab())
        .unwrap_or(0);
    let labels = RightTab::ALL
        .iter()
        .map(|tab| Line::from(format!(" {} {} ", tab.number(), tab.label())))
        .collect::<Vec<_>>();

    let tabs = Tabs::new(labels)
        .select(selected)
        .divider(Span::styled("|", Style::default().fg(Color::DarkGray)))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Right Tabs")
                .border_style(pane_border_style(focus != Focus::Left, Color::Cyan)),
        );

    frame.render_widget(tabs, area);
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
        keybind_row("Ctrl+l", "focus right column"),
        keybind_row("Ctrl+j / Ctrl+k", "move window focus"),
        keybind_row("Shift+H / Shift+L", "previous / next right tab"),
        Line::from(""),
        keybind_section("LEFT BROWSER"),
        keybind_row("j/k or Up/Down", "move selection"),
        keybind_row("Enter", "open dir or select video"),
        keybind_row("h/-", "parent directory"),
        keybind_row("_", "initial directory"),
        keybind_row("d", "delete selected file (confirm modal)"),
        keybind_row("r", "refresh listing"),
        Line::from(""),
        keybind_section("TRIM PANEL"),
        keybind_row("Tab / Shift+Tab", "move through time pieces and fields"),
        keybind_row("Space", "toggle remove-audio checkbox"),
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

fn render_delete_confirm_modal(frame: &mut Frame, app: &App) {
    let Some((name, path)) = app.pending_delete_target() else {
        return;
    };

    let outer = frame.area();
    let [vertical] = Layout::vertical([Constraint::Percentage(42)])
        .flex(ratatui::layout::Flex::Center)
        .areas(outer);
    let [popup] = Layout::horizontal([Constraint::Percentage(68)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::styled(
            "Delete this file?",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("Name: {name}")),
        Line::from(format!("Path: {}", path.display())),
        Line::from(""),
        Line::from("This cannot be undone."),
        Line::from(""),
        Line::from("Press y or Enter to confirm."),
        Line::from("Press n or Esc to cancel."),
    ];

    let popup_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm Delete")
                .border_style(pane_border_style(true, Color::LightRed)),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

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

pub(super) fn pane_border_style(is_focused: bool, focused_color: Color) -> Style {
    if is_focused {
        Style::default()
            .fg(focused_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
