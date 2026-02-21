// Application entrypoint.
// - Parses CLI startup arguments.
// - Owns the crossterm event loop and maps key events to App actions.
// - Delegates all drawing to the UI layer each frame.
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

        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            if let Event::Paste(text) = event {
                handle_paste_event(&mut app, focus, &text);
                continue;
            }

            if let Event::Key(key) = event && key.kind == KeyEventKind::Press {
                if app.has_pending_delete() {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break Ok(());
                    }

                    match key.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.confirm_pending_delete()
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.cancel_pending_delete()
                        }
                        _ => {}
                    }
                    continue;
                }

                if key.code == KeyCode::Char('?') && !is_text_input_focus(&app, focus) {
                    app.toggle_keybinds();
                    continue;
                }

                if app.show_keybinds {
                    if key.code == KeyCode::Esc {
                        app.hide_keybinds();
                    }
                    continue;
                }

                if key.modifiers == KeyModifiers::SHIFT {
                    match key.code {
                        KeyCode::Char('H') => {
                            app.select_previous_right_tab();
                            focus = Focus::RightTop;
                            continue;
                        }
                        KeyCode::Char('L') => {
                            app.select_next_right_tab();
                            focus = Focus::RightTop;
                            continue;
                        }
                        _ => {}
                    }
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
                        KeyCode::Char('u') if focus == Focus::RightBottom => match app.right_tab()
                        {
                            RightTab::Editor => app.page_ffmpeg_output_up(),
                            RightTab::Downloader => app.page_downloader_output_up(),
                        },
                        KeyCode::Char('d') | KeyCode::Char('p') if focus == Focus::RightBottom => {
                            match app.right_tab() {
                                RightTab::Editor => app.page_ffmpeg_output_down(),
                                RightTab::Downloader => app.page_downloader_output_down(),
                            }
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
                    && !app.should_treat_digit_as_editor_input(focus)
                    && app.select_right_tab_by_number(tab_number)
                {
                    focus = Focus::RightTop;
                    continue;
                }

                match focus {
                    Focus::Left => match key.code {
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::Enter => {
                            if app.activate_selected_entry()? {
                                focus = Focus::RightTop;
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Char('-') => app.go_parent_dir()?,
                        KeyCode::Char('_') => app.go_initial_dir()?,
                        KeyCode::Char('d') => app.request_delete_selected_entry(),
                        KeyCode::Char('x') => app.open_selected_with_system_default(),
                        KeyCode::Char('r') => app.reload()?,
                        _ => {}
                    },
                    Focus::RightTop => match app.right_tab() {
                        RightTab::Editor => match key.code {
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
                            KeyCode::Enter => app.run_editor_export(),
                            KeyCode::Backspace => app.backspace_active_input(),
                            KeyCode::Char(ch) => app.push_active_input_char(ch),
                            _ => {}
                        },
                        RightTab::Downloader => match key.code {
                            KeyCode::Enter => app.run_downloader_download(),
                            KeyCode::Right => app.move_downloader_cursor_right(),
                            KeyCode::Left => app.move_downloader_cursor_left(),
                            KeyCode::Backspace => app.backspace_downloader_url(),
                            KeyCode::Char(ch) => app.push_downloader_url_char(ch),
                            _ => {}
                        },
                    },
                    Focus::RightBottom => match app.right_tab() {
                        RightTab::Editor => match key.code {
                            KeyCode::Down | KeyCode::Char('j') => app.scroll_ffmpeg_output_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.scroll_ffmpeg_output_up(),
                            _ => {}
                        },
                        RightTab::Downloader => match key.code {
                            KeyCode::Down | KeyCode::Char('j') => app.scroll_downloader_output_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.scroll_downloader_output_up(),
                            _ => {}
                        },
                    },
                }
            }
        }
    }
}

fn handle_paste_event(app: &mut App, focus: Focus, text: &str) {
    if app.has_pending_delete() || app.show_keybinds || focus != Focus::RightTop {
        return;
    }

    let sanitized = text.chars().filter(|ch| *ch != '\n' && *ch != '\r');
    match app.right_tab() {
        RightTab::Downloader => {
            for ch in sanitized {
                app.push_downloader_url_char(ch);
            }
        }
        RightTab::Editor => {
            if app.active_input == InputField::Output {
                for ch in sanitized {
                    app.push_active_input_char(ch);
                }
            }
        }
    }
}

fn is_text_input_focus(app: &App, focus: Focus) -> bool {
    if focus != Focus::RightTop {
        return false;
    }

    match app.right_tab() {
        RightTab::Downloader => true,
        RightTab::Editor => app.active_input == InputField::Output,
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
