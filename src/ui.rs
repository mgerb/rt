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
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::App,
    media::is_editable_media_file,
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
        RightTab::Editor => tabs::editor::render_editor_tab(frame, app, focus, right_content),
        RightTab::Downloader => {
            tabs::downloader::render_downloader_tab(frame, app, focus, right_content)
        }
    }

    render_footer_hint(frame, footer);
    if app.show_keybinds {
        render_keybinds_popup(frame, app);
    }
    if app.has_pending_delete() {
        render_delete_confirm_modal(frame, app);
    } else if app.has_pending_cancel() {
        render_cancel_confirm_modal(frame, app);
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
                .title_top(Line::from("Tabs").left_aligned())
                .title_top(
                    Line::styled("(ctrl+n)", Style::default().fg(Color::DarkGray)).right_aligned(),
                )
                .border_style(pane_border_style(focus != Focus::Left, Color::Cyan)),
        );

    frame.render_widget(tabs, area);
}

fn render_files_pane(frame: &mut Frame, app: &App, focus: Focus, area: ratatui::layout::Rect) {
    // Account for borders and highlight symbol so selected rows stay aligned.
    let content_width = area.width.saturating_sub(4) as usize;
    let file_items = app
        .entries
        .iter()
        .map(|entry| {
            let line = format_file_row(entry, content_width);
            if is_editable_media_file(&entry.path) {
                ListItem::new(Line::styled(line, Style::default().fg(Color::LightGreen)))
            } else {
                ListItem::new(line)
            }
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border_style(focus == Focus::Left, Color::LightBlue))
        .title_top(Line::from(format!("Files: {}", app.cwd.display())).left_aligned())
        .title_top(Line::styled("(esc)", Style::default().fg(Color::DarkGray)).right_aligned());
    let inner = block.inner(area);
    let visible_rows = inner.height as usize;
    app.set_file_browser_visible_rows(visible_rows);

    let mut list_state = ListState::default();
    if !app.entries.is_empty() {
        let selected = app.selected.min(app.entries.len().saturating_sub(1));
        let centered_offset = if visible_rows == 0 {
            0
        } else {
            let max_offset = app.entries.len().saturating_sub(visible_rows);
            selected.saturating_sub(visible_rows / 2).min(max_offset)
        };
        list_state = list_state
            .with_offset(centered_offset)
            .with_selected(Some(selected));
    }

    let files = List::new(file_items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(files, area, &mut list_state);
}

fn render_keybinds_popup(frame: &mut Frame, app: &App) {
    let outer = frame.area();
    let [vertical] = Layout::vertical([Constraint::Percentage(70)])
        .flex(ratatui::layout::Flex::Center)
        .areas(outer);
    let [popup] = Layout::horizontal([Constraint::Percentage(70)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from("Press ? or Esc to close this window and focus file browser."),
        Line::from(""),
        keybind_section("GLOBAL"),
        keybind_row("?", "toggle keybinds popup"),
        keybind_row("Esc", "close modal/popup + focus file browser"),
        keybind_row("Ctrl+c", "quit app"),
        keybind_row("Up/Down or j/k", "scroll keybinds"),
        keybind_row("PgUp/PgDn or Ctrl+u/d", "page keybinds"),
        Line::from(""),
        keybind_section("WINDOW FOCUS"),
        keybind_row("Ctrl+h / Ctrl+Left", "focus left browser"),
        keybind_row("Ctrl+l / Ctrl+Right", "focus right column"),
        keybind_row("Ctrl+o", "focus tool output"),
        keybind_row("Ctrl+j/k or Ctrl+Up/Down", "move window focus"),
        keybind_row("Ctrl+n", "next right tab"),
        Line::from(""),
        keybind_section("FILE BROWSER"),
        keybind_row("j/k or Up/Down", "move selection"),
        keybind_row("PgUp/PgDn or Ctrl+u/d", "page selection"),
        keybind_row("Enter", "open dir or select media"),
        keybind_row("h/-", "parent directory"),
        keybind_row("_", "initial directory"),
        keybind_row("x", "open selected file in system default app"),
        keybind_row("d", "delete selected file (confirm modal)"),
        keybind_row("r", "refresh listing"),
        Line::from(""),
        keybind_section("EDITOR PANEL"),
        keybind_row("Tab / Shift+Tab", "move through time pieces and fields"),
        keybind_row("Space", "toggle focused checkbox"),
        keybind_row(
            "Backspace",
            "clear time piece / delete FPS/bitrate/scale/output char",
        ),
        keybind_row("Up/Down", "scroll editor form"),
        keybind_row("PgUp/PgDn or Ctrl+u/d", "page editor form"),
        keybind_row("Enter", "run editor export"),
        Line::from(""),
        keybind_section("TOOL OUTPUT"),
        keybind_row("j/k or Up/Down", "scroll output"),
        keybind_row("Ctrl+u / Ctrl+d", "page up / page down"),
        keybind_row("x", "cancel running tool"),
    ];

    let block = Block::default().borders(Borders::ALL).title("Keybinds");
    let inner = block.inner(popup);
    let visible_line_count = inner.height.max(1) as usize;
    let max_scroll_top = lines.len().saturating_sub(visible_line_count);
    let scroll_top = app.clamp_keybinds_scroll(max_scroll_top);
    let popup_widget = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left)
        .scroll((scroll_top.min(u16::MAX as usize) as u16, 0));

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

fn render_cancel_confirm_modal(frame: &mut Frame, app: &App) {
    let Some(label) = app.pending_cancel_label() else {
        return;
    };

    let outer = frame.area();
    let [vertical] = Layout::vertical([Constraint::Percentage(38)])
        .flex(ratatui::layout::Flex::Center)
        .areas(outer);
    let [popup] = Layout::horizontal([Constraint::Percentage(58)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::styled(
            "Cancel running tool?",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("Target: {label}")),
        Line::from(""),
        Line::from("Press y or Enter to confirm."),
        Line::from("Press n or Esc to keep it running."),
    ];

    let popup_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm Cancel")
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

fn file_size_label(entry: &crate::model::FileEntry) -> String {
    if entry.is_dir {
        "<DIR>".to_string()
    } else if let Some(bytes) = entry.size_bytes {
        format_size(bytes)
    } else {
        "?".to_string()
    }
}

fn format_file_row(entry: &crate::model::FileEntry, content_width: usize) -> String {
    let prefix = format!("{} ", file_type_icon(entry));
    let size = file_size_label(entry);
    let prefix_len = display_width(&prefix);
    let size_len = display_width(&size);

    let available_name_width = content_width.saturating_sub(prefix_len + size_len + 1);
    let name = truncate_middle_with_ellipsis(&entry.name, available_name_width);
    let left = format!("{prefix}{name}");
    let left_len = display_width(&left);
    let spaces = content_width.saturating_sub(left_len + size_len).max(1);
    let row = format!("{left}{}{}", " ".repeat(spaces), size);
    truncate_to_width(&row, content_width)
}

fn file_type_icon(entry: &crate::model::FileEntry) -> &'static str {
    if entry.is_dir {
        return "";
    }

    let ext = entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "mpeg" | "mpg" | "wmv" | "flv") => {
            ""
        }
        Some("mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a") => "",
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg") => "",
        Some("zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar") => "",
        Some("pdf") => "",
        Some("md" | "txt" | "rtf") => "",
        Some("rs" | "toml" | "json" | "yaml" | "yml" | "ts" | "js" | "py" | "go" | "java") => "󰈙",
        _ => "",
    }
}

fn truncate_middle_with_ellipsis(value: &str, max_chars: usize) -> String {
    let width = display_width(value);
    if width <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep_total = max_chars.saturating_sub(3);
    let keep_left = keep_total / 2;
    let keep_right = keep_total.saturating_sub(keep_left);
    let left = take_prefix_width(value, keep_left);
    let right = take_suffix_width(value, keep_right);

    truncate_to_width(&format!("{left}...{right}"), max_chars)
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn truncate_to_width(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut width = 0;
    for ch in value.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch_width > 0 && width + ch_width > max_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result
}

fn take_prefix_width(value: &str, max_width: usize) -> String {
    truncate_to_width(value, max_width)
}

fn take_suffix_width(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut suffix = Vec::new();
    let mut width = 0;
    for ch in value.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch_width > 0 && width + ch_width > max_width {
            break;
        }
        suffix.push(ch);
        width += ch_width;
    }
    suffix.into_iter().rev().collect()
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;

    if bytes_f >= GB {
        format!("{:.1}G", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1}M", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1}K", bytes_f / KB)
    } else {
        format!("{bytes}B")
    }
}
