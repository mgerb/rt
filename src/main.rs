use std::{
    env,
    fs,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Left,
    Right,
}

#[derive(Debug)]
struct App {
    cwd: PathBuf,
    initial_dir: PathBuf,
    entries: Vec<FileEntry>,
    selected: usize,
}

impl App {
    fn new() -> io::Result<Self> {
        let cwd = env::current_dir()?;
        let entries = read_entries(&cwd)?;
        Ok(Self {
            cwd: cwd.clone(),
            initial_dir: cwd,
            entries,
            selected: 0,
        })
    }

    fn next(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }

    fn previous(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected == 0 {
            self.selected = self.entries.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn reload(&mut self) -> io::Result<()> {
        self.entries = read_entries(&self.cwd)?;
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.entries.len() {
            self.selected = self.entries.len() - 1;
        }
        Ok(())
    }

    fn change_dir(&mut self, new_cwd: PathBuf) -> io::Result<()> {
        let entries = read_entries(&new_cwd)?;
        self.cwd = new_cwd;
        self.entries = entries;
        self.selected = 0;
        Ok(())
    }

    fn enter_selected_dir(&mut self) -> io::Result<()> {
        let Some(path) = self
            .selected_entry()
            .and_then(|entry| entry.is_dir.then(|| entry.path.clone()))
        else {
            return Ok(());
        };
        self.change_dir(path)
    }

    fn go_parent_dir(&mut self) -> io::Result<()> {
        let Some(parent) = self.cwd.parent() else {
            return Ok(());
        };
        self.change_dir(parent.to_path_buf())
    }

    fn go_initial_dir(&mut self) -> io::Result<()> {
        self.change_dir(self.initial_dir.clone())
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }
}

fn read_entries(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            FileEntry { name, path, is_dir }
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| (!entry.is_dir, entry.name.to_ascii_lowercase()));
    Ok(entries)
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let mut app = App::new()?;
    let mut focus = Focus::Left;

    loop {
        terminal.draw(|frame| ui(frame, &app, focus))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('h') => focus = Focus::Left,
                    KeyCode::Char('l') => focus = Focus::Right,
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                KeyCode::Char('r') => app.reload()?,
                KeyCode::Backspace if focus == Focus::Right => focus = Focus::Left,
                _ if focus == Focus::Left => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.previous(),
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => app.enter_selected_dir()?,
                    KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => app.go_parent_dir()?,
                    KeyCode::Char('-') => app.go_parent_dir()?,
                    KeyCode::Char('_') => app.go_initial_dir()?,
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn ui(frame: &mut Frame, app: &App, focus: Focus) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)]).areas(frame.area());

    let file_items = app
        .entries
        .iter()
        .map(|entry| {
            let prefix = if entry.is_dir { "[D] " } else { "    " };
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
    frame.render_stateful_widget(files, left, &mut list_state);

    let selected = app
        .selected_entry()
        .map(|entry| entry.path.display().to_string())
        .unwrap_or_else(|| "No files found".to_string());
    let details = Paragraph::new(vec![
        Line::from("Hello, world from ratatui!").alignment(Alignment::Center),
        Line::from("Left column is a file browser.").alignment(Alignment::Center),
        Line::from("Use Up/Down or j/k to move.").alignment(Alignment::Center),
        Line::from("Enter/Right/l opens directory.").alignment(Alignment::Center),
        Line::from("- (or Backspace/Left/h) goes to parent.").alignment(Alignment::Center),
        Line::from("_ goes to initial directory.").alignment(Alignment::Center),
        Line::from("Ctrl+h / Ctrl+l switches columns.").alignment(Alignment::Center),
        Line::from("Press r to refresh, q or Esc to quit.").alignment(Alignment::Center),
        Line::from(format!(
            "Active column: {}",
            if focus == Focus::Left { "left" } else { "right" }
        ))
        .alignment(Alignment::Center),
        Line::from(format!("Selected: {selected}")).alignment(Alignment::Center),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(if focus == Focus::Right {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .title("Hello TUI"),
    )
    .alignment(Alignment::Center);

    frame.render_widget(details, right);
}
