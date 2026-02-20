mod app;
mod media;
mod model;
mod ui;

use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::App;
use model::Focus;

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
        terminal.draw(|frame| ui::render(frame, &app, focus))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if key.code == KeyCode::Char('?') {
                app.toggle_keybinds();
                continue;
            }

            if app.show_keybinds {
                if key.code == KeyCode::Esc {
                    app.hide_keybinds();
                }
                continue;
            }

            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('h') => focus = Focus::Left,
                    KeyCode::Char('l') => {
                        if focus == Focus::Left {
                            focus = Focus::RightTop;
                        }
                    }
                    KeyCode::Char('j') => focus = focus.next_window(),
                    KeyCode::Char('k') => focus = focus.previous_window(),
                    KeyCode::Char('o') => {
                        focus = Focus::RightTop;
                        app.focus_output_name();
                    }
                    KeyCode::Char('c') => break Ok(()),
                    _ => {}
                }
                continue;
            }

            if key.code == KeyCode::Esc {
                break Ok(());
            }

            match focus {
                Focus::Left => match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.previous(),
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        if app.activate_selected_entry()? {
                            focus = Focus::RightTop;
                        }
                    }
                    KeyCode::Backspace
                    | KeyCode::Left
                    | KeyCode::Char('h')
                    | KeyCode::Char('-') => app.go_parent_dir()?,
                    KeyCode::Char('_') => app.go_initial_dir()?,
                    KeyCode::Char('r') => app.reload()?,
                    _ => {}
                },
                Focus::RightTop => match key.code {
                    KeyCode::Tab => app.next_input(),
                    KeyCode::BackTab => app.previous_input(),
                    KeyCode::Right | KeyCode::Char('l') => app.move_cursor_right(),
                    KeyCode::Left | KeyCode::Char('h') => app.move_cursor_left(),
                    KeyCode::Enter => app.trim_selected_video(),
                    KeyCode::Backspace => app.backspace_active_input(),
                    KeyCode::Char(ch) => app.push_active_input_char(ch),
                    _ => {}
                },
                Focus::RightBottom => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_ffmpeg_output_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_ffmpeg_output_up(),
                    _ => {}
                },
            }
        }
    }
}
