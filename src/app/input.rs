// Trim-form input editing logic.
// - Implements Tab/Shift+Tab traversal across time/output fields.
// - Handles cursor movement and character insert/delete in editable fields.
// - Keeps output names/extensions normalized and collision-safe.
use std::path::Path;

use crate::{
    media::{
        OUTPUT_FORMATS, enforce_output_extension, next_available_output_path,
        output_path_without_numbered_suffix, resolve_output_path,
    },
    model::InputField,
};

use super::App;

impl App {
    pub fn next_input(&mut self) {
        match self.active_input {
            InputField::Start => {
                if self.start_part < 2 {
                    self.start_part += 1;
                } else {
                    self.active_input = InputField::End;
                    self.end_part = 0;
                }
            }
            InputField::End => {
                if self.end_part < 2 {
                    self.end_part += 1;
                } else {
                    self.active_input = InputField::Format;
                }
            }
            InputField::Format => {
                self.active_input = InputField::Fps;
                self.output_fps_cursor = self.output_fps.chars().count();
            }
            InputField::Fps => {
                self.active_input = InputField::Bitrate;
                self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
            }
            InputField::Bitrate => {
                self.active_input = InputField::RemoveAudio;
            }
            InputField::RemoveAudio => {
                self.active_input = InputField::Output;
                self.output_cursor = self.output_name.chars().count();
            }
            InputField::Output => {
                self.active_input = InputField::Start;
                self.start_part = 0;
            }
        }
    }

    pub fn previous_input(&mut self) {
        match self.active_input {
            InputField::Start => {
                if self.start_part > 0 {
                    self.start_part -= 1;
                } else {
                    self.active_input = InputField::Output;
                    self.output_cursor = self.output_name.chars().count();
                }
            }
            InputField::End => {
                if self.end_part > 0 {
                    self.end_part -= 1;
                } else {
                    self.active_input = InputField::Start;
                    self.start_part = 2;
                }
            }
            InputField::Format => {
                self.active_input = InputField::End;
                self.end_part = 2;
            }
            InputField::Fps => self.active_input = InputField::Format,
            InputField::Bitrate => {
                self.active_input = InputField::Fps;
                self.output_fps_cursor = self.output_fps.chars().count();
            }
            InputField::RemoveAudio => {
                self.active_input = InputField::Bitrate;
                self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
            }
            InputField::Output => self.active_input = InputField::RemoveAudio,
        }
    }

    pub fn move_cursor_left(&mut self) {
        match self.active_input {
            InputField::Format => self.select_previous_output_format(),
            InputField::Fps => self.output_fps_cursor = self.output_fps_cursor.saturating_sub(1),
            InputField::Bitrate => {
                self.output_bitrate_cursor = self.output_bitrate_cursor.saturating_sub(1)
            }
            InputField::Output => self.output_cursor = self.output_cursor.saturating_sub(1),
            _ => {}
        }
    }

    pub fn move_cursor_right(&mut self) {
        match self.active_input {
            InputField::Format => self.select_next_output_format(),
            InputField::Fps => {
                let max = self.output_fps.chars().count();
                self.output_fps_cursor = (self.output_fps_cursor + 1).min(max);
            }
            InputField::Bitrate => {
                let max = self.output_bitrate_kbps.chars().count();
                self.output_bitrate_cursor = (self.output_bitrate_cursor + 1).min(max);
            }
            InputField::Output => {
                let max = self.output_name.chars().count();
                self.output_cursor = (self.output_cursor + 1).min(max);
            }
            _ => {}
        }
    }

    pub fn toggle_remove_audio(&mut self) {
        self.remove_audio = !self.remove_audio;
    }

    pub fn push_active_input_char(&mut self, ch: char) {
        match self.active_input {
            InputField::Start => {
                if ch.is_ascii_digit() {
                    self.start_time.push_digit_to_part(self.start_part, ch);
                }
            }
            InputField::End => {
                if ch.is_ascii_digit() {
                    self.end_time.push_digit_to_part(self.end_part, ch);
                }
            }
            InputField::Format => {}
            InputField::Fps => {
                if ch.is_ascii_digit() || (ch == '.' && !self.output_fps.contains('.')) {
                    let byte_index = byte_index_for_char(&self.output_fps, self.output_fps_cursor);
                    self.output_fps.insert(byte_index, ch);
                    self.output_fps_cursor += 1;
                }
            }
            InputField::Bitrate => {
                if ch.is_ascii_digit() {
                    let byte_index =
                        byte_index_for_char(&self.output_bitrate_kbps, self.output_bitrate_cursor);
                    self.output_bitrate_kbps.insert(byte_index, ch);
                    self.output_bitrate_cursor += 1;
                }
            }
            InputField::RemoveAudio => {
                if ch == ' ' {
                    self.toggle_remove_audio();
                }
            }
            InputField::Output => {
                let byte_index = byte_index_for_char(&self.output_name, self.output_cursor);
                self.output_name.insert(byte_index, ch);
                self.output_cursor += 1;
            }
        }
    }

    pub fn backspace_active_input(&mut self) {
        match self.active_input {
            InputField::Start => {
                self.start_time.clear_part(self.start_part);
            }
            InputField::End => {
                self.end_time.clear_part(self.end_part);
            }
            InputField::Format => {}
            InputField::Fps => {
                if self.output_fps_cursor == 0 {
                    return;
                }
                let remove_char_index = self.output_fps_cursor - 1;
                let start = byte_index_for_char(&self.output_fps, remove_char_index);
                let end = byte_index_for_char(&self.output_fps, remove_char_index + 1);
                self.output_fps.replace_range(start..end, "");
                self.output_fps_cursor -= 1;
            }
            InputField::Bitrate => {
                if self.output_bitrate_cursor == 0 {
                    return;
                }
                let remove_char_index = self.output_bitrate_cursor - 1;
                let start = byte_index_for_char(&self.output_bitrate_kbps, remove_char_index);
                let end = byte_index_for_char(&self.output_bitrate_kbps, remove_char_index + 1);
                self.output_bitrate_kbps.replace_range(start..end, "");
                self.output_bitrate_cursor -= 1;
            }
            InputField::RemoveAudio => {}
            InputField::Output => {
                if self.output_cursor == 0 {
                    return;
                }
                let remove_char_index = self.output_cursor - 1;
                let start = byte_index_for_char(&self.output_name, remove_char_index);
                let end = byte_index_for_char(&self.output_name, remove_char_index + 1);
                self.output_name.replace_range(start..end, "");
                self.output_cursor -= 1;
            }
        }
    }

    fn select_previous_output_format(&mut self) {
        let current_index = OUTPUT_FORMATS
            .iter()
            .position(|format| *format == self.output_format)
            .unwrap_or(0);
        let next_index = if current_index == 0 {
            OUTPUT_FORMATS.len() - 1
        } else {
            current_index - 1
        };
        self.output_format = OUTPUT_FORMATS[next_index];
        self.sync_output_extension_to_selected_format();
    }

    fn select_next_output_format(&mut self) {
        let current_index = OUTPUT_FORMATS
            .iter()
            .position(|format| *format == self.output_format)
            .unwrap_or(0);
        let next_index = (current_index + 1) % OUTPUT_FORMATS.len();
        self.output_format = OUTPUT_FORMATS[next_index];
        self.sync_output_extension_to_selected_format();
    }

    fn sync_output_extension_to_selected_format(&mut self) {
        if self.output_name.trim().is_empty() {
            return;
        }

        if let Some(input_path) = self.selected_video.clone() {
            self.sync_output_name_to_available_for_path(&input_path);
            return;
        }

        self.output_name = enforce_output_extension(&self.output_name, self.output_format);
        self.output_cursor = self.output_cursor.min(self.output_name.chars().count());
    }

    pub(super) fn sync_output_name_to_available_for_path(&mut self, input_path: &Path) {
        let requested_output_name = enforce_output_extension(&self.output_name, self.output_format);
        let requested_output_path = resolve_output_path(input_path, &requested_output_name);
        let normalized_output_path = output_path_without_numbered_suffix(&requested_output_path);
        let available_output_path = next_available_output_path(&normalized_output_path);
        self.sync_output_name_with_path(&requested_output_name, &available_output_path);
    }

    pub(super) fn sync_output_name_with_path(
        &mut self,
        requested_output_name: &str,
        resolved_path: &Path,
    ) {
        let requested = Path::new(requested_output_name);
        let requested_has_path = requested.is_absolute()
            || requested_output_name.contains('/')
            || requested_output_name.contains('\\');

        self.output_name = if requested_has_path {
            resolved_path.display().to_string()
        } else {
            resolved_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| resolved_path.display().to_string())
        };
        self.output_cursor = self.output_name.chars().count();
    }
}

pub(super) fn byte_index_for_char(input: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }

    input
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(input.len())
}
