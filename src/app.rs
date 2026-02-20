use std::{
    env, fs,
    fs::OpenOptions,
    io,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    media::{
        VideoStats, collect_ffmpeg_lines, default_output_name, is_video_file, probe_video_stats,
        probe_video_times, resolve_output_path, shell_quote, summarize_ffmpeg_error,
    },
    model::{FileEntry, InputField, TimeInput, VideoBounds},
};

#[derive(Debug)]
pub struct App {
    pub(crate) cwd: PathBuf,
    initial_dir: PathBuf,
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) selected: usize,
    pub(crate) selected_video: Option<PathBuf>,
    pub(crate) start_time: TimeInput,
    pub(crate) end_time: TimeInput,
    pub(crate) output_name: String,
    pub(crate) active_input: InputField,
    pub(crate) start_cursor: usize,
    pub(crate) end_cursor: usize,
    pub(crate) output_cursor: usize,
    pub(crate) selected_video_stats: Option<VideoStats>,
    selected_video_bounds: Option<VideoBounds>,
    pub(crate) status_message: String,
    pub(crate) ffmpeg_output: Vec<String>,
    pub(crate) ffmpeg_scroll: usize,
    pub(crate) show_keybinds: bool,
}

impl App {
    pub fn new() -> io::Result<Self> {
        let cwd = env::current_dir()?;
        let entries = read_entries(&cwd)?;

        Ok(Self {
            cwd: cwd.clone(),
            initial_dir: cwd,
            entries,
            selected: 0,
            selected_video: None,
            start_time: TimeInput::zero(),
            end_time: TimeInput::zero(),
            output_name: String::new(),
            active_input: InputField::Start,
            start_cursor: 0,
            end_cursor: 0,
            output_cursor: 0,
            selected_video_stats: None,
            selected_video_bounds: None,
            status_message: "Select a video file in the left pane.".to_string(),
            ffmpeg_output: vec!["ffmpeg output will appear here after trimming.".to_string()],
            ffmpeg_scroll: 0,
            show_keybinds: false,
        })
    }

    pub fn toggle_keybinds(&mut self) {
        self.show_keybinds = !self.show_keybinds;
    }

    pub fn hide_keybinds(&mut self) {
        self.show_keybinds = false;
    }

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

    pub fn go_parent_dir(&mut self) -> io::Result<()> {
        let Some(parent) = self.cwd.parent() else {
            return Ok(());
        };
        self.change_dir(parent.to_path_buf())
    }

    pub fn go_initial_dir(&mut self) -> io::Result<()> {
        self.change_dir(self.initial_dir.clone())
    }

    pub fn next_input(&mut self) {
        self.active_input = self.active_input.next();
    }

    pub fn previous_input(&mut self) {
        self.active_input = self.active_input.previous();
    }

    pub fn focus_output_name(&mut self) {
        self.active_input = InputField::Output;
        self.output_cursor = self.output_name.chars().count();
    }

    pub fn move_cursor_left(&mut self) {
        match self.active_input {
            InputField::Start => self.start_cursor = self.start_cursor.saturating_sub(1),
            InputField::End => self.end_cursor = self.end_cursor.saturating_sub(1),
            InputField::Output => self.output_cursor = self.output_cursor.saturating_sub(1),
        }
    }

    pub fn move_cursor_right(&mut self) {
        match self.active_input {
            InputField::Start => self.start_cursor = (self.start_cursor + 1).min(5),
            InputField::End => self.end_cursor = (self.end_cursor + 1).min(5),
            InputField::Output => {
                let max = self.output_name.chars().count();
                self.output_cursor = (self.output_cursor + 1).min(max);
            }
        }
    }

    pub fn push_active_input_char(&mut self, ch: char) {
        match self.active_input {
            InputField::Start => {
                if ch.is_ascii_digit() {
                    self.start_time.set_digit_at(self.start_cursor, ch);
                    self.start_cursor = (self.start_cursor + 1).min(5);
                }
            }
            InputField::End => {
                if ch.is_ascii_digit() {
                    self.end_time.set_digit_at(self.end_cursor, ch);
                    self.end_cursor = (self.end_cursor + 1).min(5);
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
                if self.start_cursor > 0 {
                    self.start_cursor -= 1;
                }
                self.start_time.set_digit_at(self.start_cursor, '0');
            }
            InputField::End => {
                if self.end_cursor > 0 {
                    self.end_cursor -= 1;
                }
                self.end_time.set_digit_at(self.end_cursor, '0');
            }
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

    pub fn trim_selected_video(&mut self) {
        let Some(input_path) = self.selected_video.clone() else {
            self.status_message = "No video selected. Choose one in the left pane.".to_string();
            return;
        };

        if !self.start_time.has_valid_minute_second_range()
            || !self.end_time.has_valid_minute_second_range()
        {
            self.status_message = "Minutes and seconds must be between 00 and 59.".to_string();
            return;
        }

        let start_seconds = self.start_time.to_seconds();
        let end_seconds = self.end_time.to_seconds();
        let start = self.start_time.to_ffmpeg_timestamp();
        let output = self.output_name.trim();

        if let Some(bounds) = self.selected_video_bounds {
            if start_seconds < bounds.start_seconds {
                self.status_message = format!(
                    "Start time must be >= {}.",
                    TimeInput::from_seconds(bounds.start_seconds as f64).to_ffmpeg_timestamp()
                );
                return;
            }
            if start_seconds >= bounds.end_seconds {
                self.status_message = format!(
                    "Start time must be < {}.",
                    TimeInput::from_seconds(bounds.end_seconds as f64).to_ffmpeg_timestamp()
                );
                return;
            }
            if end_seconds > bounds.end_seconds {
                self.status_message = format!(
                    "End time must be <= {}.",
                    TimeInput::from_seconds(bounds.end_seconds as f64).to_ffmpeg_timestamp()
                );
                return;
            }
        }

        if end_seconds <= start_seconds {
            self.status_message = "End time must be greater than start time.".to_string();
            return;
        }

        let clip_duration = end_seconds - start_seconds;
        if output.is_empty() {
            self.status_message = "Output file name is required.".to_string();
            return;
        }

        let output_path = resolve_output_path(&input_path, output);
        self.status_message = format!("Running ffmpeg -> {}", output_path.display());
        self.ffmpeg_scroll = 0;

        let ffmpeg_args = vec![
            "-y".to_string(),
            "-hide_banner".to_string(),
            "-ss".to_string(),
            start.clone(),
            "-i".to_string(),
            input_path.display().to_string(),
            "-t".to_string(),
            clip_duration.to_string(),
            "-map".to_string(),
            "0:v:0?".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            "veryfast".to_string(),
            "-crf".to_string(),
            "20".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "192k".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            output_path.display().to_string(),
        ];

        let command_line = format!(
            "ffmpeg {}",
            ffmpeg_args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );

        self.ffmpeg_output = vec![format!("$ {command_line}"), "Running...".to_string()];
        let result = Command::new("ffmpeg").args(&ffmpeg_args).output();

        match result {
            Ok(output_data) if output_data.status.success() => {
                self.ffmpeg_output =
                    collect_ffmpeg_lines(&command_line, &output_data.stdout, &output_data.stderr);
                self.ffmpeg_scroll = 0;

                match self.append_ffmpeg_run_log(
                    &command_line,
                    output_data.status.code(),
                    &output_data.stdout,
                    &output_data.stderr,
                    None,
                ) {
                    Ok(log_path) => {
                        self.status_message = format!(
                            "Created clip: {} (log: {})",
                            output_path.display(),
                            log_path.display()
                        );
                    }
                    Err(log_err) => {
                        self.status_message = format!(
                            "Created clip: {} (log write failed: {log_err})",
                            output_path.display()
                        );
                    }
                }
            }
            Ok(output_data) => {
                self.ffmpeg_output =
                    collect_ffmpeg_lines(&command_line, &output_data.stdout, &output_data.stderr);
                self.ffmpeg_scroll = 0;
                let stderr = String::from_utf8_lossy(&output_data.stderr);
                let detail = summarize_ffmpeg_error(&stderr);

                match self.append_ffmpeg_run_log(
                    &command_line,
                    output_data.status.code(),
                    &output_data.stdout,
                    &output_data.stderr,
                    None,
                ) {
                    Ok(log_path) => {
                        self.status_message =
                            format!("ffmpeg failed: {detail} (log: {})", log_path.display());
                    }
                    Err(log_err) => {
                        self.status_message =
                            format!("ffmpeg failed: {detail} (log write failed: {log_err})");
                    }
                }
            }
            Err(err) => {
                self.ffmpeg_output = vec![
                    format!("$ {command_line}"),
                    format!("Failed to start ffmpeg: {err}"),
                ];
                self.ffmpeg_scroll = 0;

                match self.append_ffmpeg_run_log(
                    &command_line,
                    None,
                    &[],
                    &[],
                    Some(&err.to_string()),
                ) {
                    Ok(log_path) => {
                        self.status_message = format!(
                            "Failed to start ffmpeg: {err} (log: {})",
                            log_path.display()
                        );
                    }
                    Err(log_err) => {
                        self.status_message =
                            format!("Failed to start ffmpeg: {err} (log write failed: {log_err})");
                    }
                }
            }
        }
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

    fn select_video(&mut self, path: PathBuf) {
        self.output_name = default_output_name(&path);
        self.selected_video_stats = probe_video_stats(&path).ok();

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
        self.start_cursor = 0;
        self.end_cursor = 0;
        self.output_cursor = self.output_name.chars().count();
        self.selected_video = Some(path);
    }

    fn append_ffmpeg_run_log(
        &self,
        command_line: &str,
        exit_code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
        launch_error: Option<&str>,
    ) -> io::Result<PathBuf> {
        let log_path = self.initial_dir.join("ffmpeg_runs.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        writeln!(file, "=== ffmpeg run @ {timestamp} ===")?;
        writeln!(file, "command: {command_line}")?;
        match exit_code {
            Some(code) => writeln!(file, "exit_code: {code}")?,
            None => writeln!(file, "exit_code: <none>")?,
        }

        if let Some(err) = launch_error {
            writeln!(file, "launch_error: {err}")?;
        }

        writeln!(file, "--- stderr ---")?;
        file.write_all(stderr)?;
        if !stderr.ends_with(b"\n") {
            writeln!(file)?;
        }

        writeln!(file, "--- stdout ---")?;
        file.write_all(stdout)?;
        if !stdout.ends_with(b"\n") {
            writeln!(file)?;
        }

        writeln!(file, "=== end ===")?;
        writeln!(file)?;

        Ok(log_path)
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

fn byte_index_for_char(input: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }

    input
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(input.len())
}
