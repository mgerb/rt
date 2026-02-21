// File-browser behavior for the left pane.
// - Reads/sorts directory entries and manages selection movement.
// - Handles directory navigation and entry activation.
// - Starts delete confirmation flow and removes files after confirmation.
// - Populates trim defaults when a video file is selected.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    media::{
        default_output_name, is_video_file, output_format_for_path, probe_video_stats,
        probe_video_times,
    },
    model::{FileEntry, InputField, TimeInput},
};

use super::{App, PendingDelete, trim::default_output_fps};

impl App {
    pub fn next(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }

    pub fn previous(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected == 0 {
            self.selected = self.entries.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn reload(&mut self) -> io::Result<()> {
        self.entries = read_entries(&self.cwd)?;
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.entries.len() {
            self.selected = self.entries.len() - 1;
        }
        Ok(())
    }

    pub fn activate_selected_entry(&mut self) -> io::Result<bool> {
        let Some(entry) = self.selected_entry().cloned() else {
            return Ok(false);
        };

        if entry.is_dir {
            self.change_dir(entry.path)?;
            return Ok(false);
        }

        if is_video_file(&entry.path) {
            self.select_video(entry.path);
            return Ok(true);
        }

        self.status_message = format!("Not a video file: {}", entry.name);
        Ok(false)
    }

    pub fn request_delete_selected_entry(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = "No entry selected.".to_string();
            return;
        };

        if entry.is_dir {
            self.status_message = "Delete is only supported for files.".to_string();
            return;
        }

        self.pending_delete = Some(PendingDelete {
            name: entry.name,
            path: entry.path,
        });
    }

    pub fn cancel_pending_delete(&mut self) {
        self.pending_delete = None;
    }

    pub fn confirm_pending_delete(&mut self) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };

        match fs::remove_file(&pending.path) {
            Ok(()) => {
                self.clear_selected_video_if_matches(&pending.path);
                if let Err(err) = self.reload() {
                    self.status_message = format!(
                        "Deleted {}, but failed to refresh browser: {err}",
                        pending.name
                    );
                    return;
                }
                self.status_message = format!("Deleted file: {}", pending.name);
            }
            Err(err) => {
                self.status_message = format!("Failed to delete {}: {err}", pending.name);
            }
        }
    }

    pub fn go_parent_dir(&mut self) -> io::Result<()> {
        let Some(parent) = self.cwd.parent() else {
            return Ok(());
        };
        self.change_dir(parent.to_path_buf())
    }

    pub fn go_initial_dir(&mut self) -> io::Result<()> {
        self.change_dir(self.initial_dir.clone())
    }

    pub fn scroll_ffmpeg_output_down(&mut self) {
        let max_scroll = self.ffmpeg_output.len().saturating_sub(1);
        if self.ffmpeg_scroll < max_scroll {
            self.ffmpeg_scroll += 1;
        }
    }

    pub fn scroll_ffmpeg_output_up(&mut self) {
        self.ffmpeg_scroll = self.ffmpeg_scroll.saturating_sub(1);
    }

    fn change_dir(&mut self, new_cwd: PathBuf) -> io::Result<()> {
        let entries = read_entries(&new_cwd)?;
        self.cwd = new_cwd;
        self.entries = entries;
        self.selected = 0;
        Ok(())
    }

    pub(super) fn refresh_file_browser_after_save(&mut self, output_path: &Path) -> io::Result<()> {
        self.reload()?;

        let output_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
        if output_dir != self.cwd {
            return Ok(());
        }

        let Some(output_name) = output_path.file_name().and_then(|name| name.to_str()) else {
            return Ok(());
        };

        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.name == output_name)
        {
            self.selected = index;
        }

        Ok(())
    }

    fn select_video(&mut self, path: PathBuf) {
        self.output_name = default_output_name(&path);
        self.output_format = output_format_for_path(&path);
        self.selected_video_stats = probe_video_stats(&path).ok();
        self.output_fps = default_output_fps(self.selected_video_stats.as_ref());
        self.output_fps_cursor = self.output_fps.chars().count();
        self.output_bitrate_kbps = default_output_bitrate_kbps(self.selected_video_stats.as_ref());
        self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
        self.remove_audio = false;
        self.sync_output_name_to_available_for_path(&path);

        match probe_video_times(&path) {
            Ok((start_time, end_time, bounds)) => {
                self.start_time = start_time;
                self.end_time = end_time;
                self.selected_video_bounds = Some(bounds);
                self.status_message = format!(
                    "Selected video: {} (range {}..={})",
                    path.display(),
                    TimeInput::from_seconds(bounds.start_seconds as f64).to_ffmpeg_timestamp(),
                    TimeInput::from_seconds(bounds.end_seconds as f64).to_ffmpeg_timestamp()
                );
            }
            Err(err) => {
                self.start_time = TimeInput::zero();
                self.end_time = TimeInput::zero();
                self.selected_video_bounds = None;
                self.status_message = format!(
                    "Selected video (ffprobe failed, using 00:00:00): {} ({err})",
                    path.display()
                );
            }
        }

        self.active_input = InputField::Start;
        self.start_part = 0;
        self.end_part = 0;
        self.output_fps_cursor = self.output_fps.chars().count();
        self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
        self.output_cursor = self.output_name.chars().count();
        self.selected_video = Some(path);
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    fn clear_selected_video_if_matches(&mut self, deleted_path: &Path) {
        if self
            .selected_video
            .as_ref()
            .is_some_and(|path| path == deleted_path)
        {
            self.selected_video = None;
            self.selected_video_stats = None;
            self.selected_video_bounds = None;
            self.start_time = TimeInput::zero();
            self.end_time = TimeInput::zero();
            self.output_name.clear();
            self.output_cursor = 0;
        }
    }
}

pub(super) fn read_entries(dir: &Path) -> io::Result<Vec<FileEntry>> {
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

fn default_output_bitrate_kbps(stats: Option<&crate::media::VideoStats>) -> String {
    stats
        .and_then(|stats| stats.bitrate_kbps)
        .map(|bitrate| bitrate.to_string())
        .unwrap_or_else(|| "8000".to_string())
}
