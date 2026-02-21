mod app;
mod media;
mod model;
mod ui;

use std::{env, io, path::PathBuf, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::App;
use model::{Focus, InputField, RightTab};

fn main() -> io::Result<()> {
    let start_dir = parse_start_dir_arg()?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, start_dir);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, start_dir: Option<PathBuf>) -> io::Result<()> {
    let mut app = App::new(start_dir)?;
    let mut focus = Focus::Left;

    loop {
        app.normalize_focus(&mut focus);
        app.tick();
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
                    KeyCode::Char('j') => focus = app.next_focus(focus),
                    KeyCode::Char('k') => focus = app.previous_focus(focus),
                    KeyCode::Char('n') => {
                        app.select_next_right_tab();
                        app.normalize_focus(&mut focus);
                    }
                    KeyCode::Char('p') => {
                        app.select_previous_right_tab();
                        app.normalize_focus(&mut focus);
                    }
                    KeyCode::Char('c') => break Ok(()),
                    _ => {}
                }
                continue;
            }

            if key.code == KeyCode::Esc {
                break Ok(());
            }

            if let Some(tab_number) = tab_number_shortcut(key.code, key.modifiers)
                && !app.should_treat_digit_as_trim_input(focus)
                && app.select_right_tab_by_number(tab_number)
            {
                app.normalize_focus(&mut focus);
                continue;
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
                Focus::RightTop => {
                    if app.right_tab() == RightTab::Trim {
                        match key.code {
                            KeyCode::Tab => app.next_input(),
                            KeyCode::BackTab => app.previous_input(),
                            KeyCode::Right => app.move_cursor_right(),
                            KeyCode::Left => app.move_cursor_left(),
                            KeyCode::Char('h') if app.active_input == InputField::Format => {
                                app.move_cursor_left()
                            }
                            KeyCode::Char('l') if app.active_input == InputField::Format => {
                                app.move_cursor_right()
                            }
                            KeyCode::Enter => app.trim_selected_video(),
                            KeyCode::Backspace => app.backspace_active_input(),
                            KeyCode::Char(ch) => app.push_active_input_char(ch),
                            _ => {}
                        }
                    }
                }
                Focus::RightBottom => {
                    if app.can_focus_right_bottom() {
                        match key.code {
                            KeyCode::Down | KeyCode::Char('j') => app.scroll_ffmpeg_output_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.scroll_ffmpeg_output_up(),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn tab_number_shortcut(code: KeyCode, modifiers: KeyModifiers) -> Option<usize> {
    if !modifiers.is_empty() {
        return None;
    }

    let KeyCode::Char(ch) = code else {
        return None;
    };
    if !ch.is_ascii_digit() {
        return None;
    }

    ch.to_digit(10).map(|value| value as usize)
}

fn parse_start_dir_arg() -> io::Result<Option<PathBuf>> {
    let mut args = env::args_os().skip(1);
    let first = args.next().map(PathBuf::from);
    if let Some(extra) = args.next() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Unexpected extra argument: {:?}. Usage: rt [start-directory]",
                extra
            ),
        ));
    }
    Ok(first)
}
