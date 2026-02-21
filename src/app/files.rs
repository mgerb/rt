// File-browser behavior for the left pane.
// - Reads/sorts directory entries and manages selection movement.
// - Handles directory navigation and entry activation.
// - Starts delete confirmation flow and removes files after confirmation.
// - Populates editor defaults when an editable media file is selected.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    media::{
        default_output_name, is_editable_media_file, output_format_for_path, probe_video_stats,
        probe_video_times,
    },
    model::{FileEntry, InputField, RightTab, TimeInput},
};

use super::{App, PendingDelete, editor::default_output_fps};

const EDITOR_FORM_PAGE_STEP: usize = 8;

impl App {
    pub fn next(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
        }
    }

    pub fn previous(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub fn page_files_down(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }

        let max_index = self.entries.len().saturating_sub(1);
        let step = self.file_browser_page_step();
        self.selected = (self.selected + step).min(max_index);
    }

    pub fn page_files_up(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }

        let step = self.file_browser_page_step();
        self.selected = self.selected.saturating_sub(step);
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

        if is_editable_media_file(&entry.path) {
            self.select_media(entry.path);
            return Ok(true);
        }

        self.status_message = format!("Not a supported media file: {}", entry.name);
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

    pub fn open_selected_with_system_default(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = "No entry selected.".to_string();
            return;
        };

        if entry.is_dir {
            self.status_message = "Use Enter to open directories in the browser.".to_string();
            return;
        }

        match open_with_system_default(&entry.path) {
            Ok(()) => {
                self.status_message = format!("Opened with system default: {}", entry.name);
            }
            Err(err) => {
                self.status_message = format!("Failed to open {}: {err}", entry.name);
            }
        }
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
        self.ffmpeg_output.scroll_down();
    }

    pub fn scroll_ffmpeg_output_up(&mut self) {
        self.ffmpeg_output.scroll_up();
    }

    pub fn page_ffmpeg_output_down(&mut self) {
        self.ffmpeg_output.page_down();
    }

    pub fn page_ffmpeg_output_up(&mut self) {
        self.ffmpeg_output.page_up();
    }

    pub fn scroll_editor_form_down(&mut self) {
        self.editor_form_scroll
            .set(self.editor_form_scroll().saturating_add(1));
    }

    pub fn scroll_editor_form_up(&mut self) {
        self.editor_form_scroll
            .set(self.editor_form_scroll().saturating_sub(1));
    }

    pub fn page_editor_form_down(&mut self) {
        self.editor_form_scroll.set(
            self.editor_form_scroll()
                .saturating_add(EDITOR_FORM_PAGE_STEP),
        );
    }

    pub fn page_editor_form_up(&mut self) {
        self.editor_form_scroll.set(
            self.editor_form_scroll()
                .saturating_sub(EDITOR_FORM_PAGE_STEP),
        );
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

    fn select_media(&mut self, path: PathBuf) {
        self.right_tab = RightTab::Editor;
        self.output_name = default_output_name(&path);
        self.output_format = output_format_for_path(&path);
        self.selected_video_stats = probe_video_stats(&path).ok();
        self.output_fps = default_output_fps(self.selected_video_stats.as_ref());
        self.output_fps_cursor = self.output_fps.chars().count();
        self.output_bitrate_kbps = default_output_bitrate_kbps(self.selected_video_stats.as_ref());
        self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
        self.output_scale_percent = "100".to_string();
        self.output_scale_percent_cursor = self.output_scale_percent.chars().count();
        self.use_gpu_encoding = self.gpu_h264_encoder_available();
        self.remove_audio = false;
        self.sync_output_name_to_available_for_path(&path);

        match probe_video_times(&path) {
            Ok((start_time, end_time, bounds)) => {
                self.start_time = start_time;
                self.end_time = end_time;
                self.selected_video_bounds = Some(bounds);
                self.status_message = format!(
                    "Selected media: {} (range {}..={})",
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
                    "Selected media (ffprobe failed, using 00:00:00): {} ({err})",
                    path.display()
                );
            }
        }

        self.active_input = InputField::Start;
        self.start_part = 0;
        self.end_part = 0;
        self.output_fps_cursor = self.output_fps.chars().count();
        self.output_bitrate_cursor = self.output_bitrate_kbps.chars().count();
        self.output_scale_percent_cursor = self.output_scale_percent.chars().count();
        self.output_cursor = self.output_name.chars().count();
        self.overwrite_fps_on_next_type = true;
        self.overwrite_bitrate_on_next_type = true;
        self.overwrite_scale_percent_on_next_type = true;
        self.editor_form_scroll.set(0);
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
            self.remove_audio = false;
            self.output_scale_percent = "100".to_string();
            self.output_scale_percent_cursor = self.output_scale_percent.chars().count();
            self.output_cursor = 0;
            self.editor_form_scroll.set(0);
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
            let size_bytes = if is_dir {
                None
            } else {
                entry.metadata().ok().map(|meta| meta.len())
            };

            FileEntry {
                name,
                path,
                is_dir,
                size_bytes,
            }
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

fn open_with_system_default(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "opening files is not supported on this platform",
    ))
}
