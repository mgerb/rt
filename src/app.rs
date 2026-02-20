use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    media::{
        OUTPUT_FORMATS, VideoStats, default_output_name, enforce_output_extension, is_video_file,
        next_available_output_path, output_format_for_path, output_path_without_numbered_suffix,
        probe_video_stats, probe_video_times, resolve_output_path, shell_quote,
        summarize_ffmpeg_error,
    },
    model::{FileEntry, InputField, TimeInput, VideoBounds},
};

pub struct App {
    pub(crate) cwd: PathBuf,
    initial_dir: PathBuf,
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) selected: usize,
    pub(crate) selected_video: Option<PathBuf>,
    pub(crate) start_time: TimeInput,
    pub(crate) end_time: TimeInput,
    pub(crate) output_format: &'static str,
    pub(crate) remove_audio: bool,
    pub(crate) output_name: String,
    pub(crate) active_input: InputField,
    pub(crate) start_part: usize,
    pub(crate) end_part: usize,
    pub(crate) output_cursor: usize,
    pub(crate) selected_video_stats: Option<VideoStats>,
    selected_video_bounds: Option<VideoBounds>,
    pub(crate) status_message: String,
    pub(crate) ffmpeg_output: Vec<String>,
    pub(crate) ffmpeg_scroll: usize,
    pub(crate) show_keybinds: bool,
    pub(crate) ffmpeg_spinner_frame: usize,
    running_trim: Option<RunningTrim>,
}

struct RunningTrim {
    child: Child,
    rx: Receiver<FfmpegEvent>,
    command_line: String,
    output_path: PathBuf,
    stdout_raw: Vec<u8>,
    stderr_raw: Vec<u8>,
    stdout_pending: Vec<u8>,
    stderr_pending: Vec<u8>,
}

#[derive(Clone, Copy)]
enum FfmpegStream {
    Stdout,
    Stderr,
}

enum FfmpegEvent {
    Chunk { stream: FfmpegStream, data: Vec<u8> },
    ReaderError { stream: FfmpegStream, error: String },
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
            output_format: OUTPUT_FORMATS[0],
            remove_audio: false,
            output_name: String::new(),
            active_input: InputField::Start,
            start_part: 0,
            end_part: 0,
            output_cursor: 0,
            selected_video_stats: None,
            selected_video_bounds: None,
            status_message: "Select a video file in the left pane.".to_string(),
            ffmpeg_output: vec!["ffmpeg output will appear here after trimming.".to_string()],
            ffmpeg_scroll: 0,
            show_keybinds: false,
            ffmpeg_spinner_frame: 0,
            running_trim: None,
        })
    }

    pub fn toggle_keybinds(&mut self) {
        self.show_keybinds = !self.show_keybinds;
    }

    pub fn hide_keybinds(&mut self) {
        self.show_keybinds = false;
    }

    pub fn tick(&mut self) {
        if self.running_trim.is_none() {
            return;
        }

        self.ffmpeg_spinner_frame = (self.ffmpeg_spinner_frame + 1) % spinner_frames().len();
        self.pump_running_trim_events();
        self.try_finish_running_trim();
    }

    pub fn ffmpeg_is_running(&self) -> bool {
        self.running_trim.is_some()
    }

    pub fn ffmpeg_spinner_glyph(&self) -> char {
        spinner_frames()[self.ffmpeg_spinner_frame]
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
            InputField::RemoveAudio => self.active_input = InputField::Format,
            InputField::Output => self.active_input = InputField::RemoveAudio,
        }
    }

    pub fn focus_output_name(&mut self) {
        self.active_input = InputField::Output;
        self.output_cursor = self.output_name.chars().count();
    }

    pub fn move_cursor_left(&mut self) {
        match self.active_input {
            InputField::Format => self.select_previous_output_format(),
            InputField::Output => self.output_cursor = self.output_cursor.saturating_sub(1),
            _ => {}
        }
    }

    pub fn move_cursor_right(&mut self) {
        match self.active_input {
            InputField::Format => self.select_next_output_format(),
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

    pub fn trim_selected_video(&mut self) {
        if self.running_trim.is_some() {
            self.status_message = "ffmpeg is already running. Wait for it to finish.".to_string();
            return;
        }

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

        let output_name = enforce_output_extension(output, self.output_format);
        self.output_name = output_name.clone();
        self.output_cursor = self.output_cursor.min(self.output_name.chars().count());

        let requested_output_path = resolve_output_path(&input_path, &output_name);
        let output_path = next_available_output_path(&requested_output_path);
        self.sync_output_name_with_path(&output_name, &output_path);
        self.status_message = format!("Running ffmpeg -> {}", output_path.display());
        self.ffmpeg_scroll = 0;

        let mut ffmpeg_args = vec![
            "-y".to_string(),
            "-hide_banner".to_string(),
            "-ss".to_string(),
            start.clone(),
            "-i".to_string(),
            input_path.display().to_string(),
            "-t".to_string(),
            clip_duration.to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
        ];

        if self.output_format == "gif" {
            ffmpeg_args.extend([
                "-map".to_string(),
                "0:v:0?".to_string(),
                "-an".to_string(),
                "-vf".to_string(),
                "fps=12".to_string(),
                "-loop".to_string(),
                "0".to_string(),
            ]);
        } else {
            ffmpeg_args.extend([
                "-map".to_string(),
                "0:v:0?".to_string(),
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "veryfast".to_string(),
                "-crf".to_string(),
                "20".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
            ]);
            if self.remove_audio {
                ffmpeg_args.push("-an".to_string());
            } else {
                ffmpeg_args.extend([
                    "-map".to_string(),
                    "0:a:0?".to_string(),
                    "-c:a".to_string(),
                    "aac".to_string(),
                    "-b:a".to_string(),
                    "192k".to_string(),
                ]);
            }
            ffmpeg_args.extend(["-movflags".to_string(), "+faststart".to_string()]);
        }

        ffmpeg_args.push(output_path.display().to_string());

        let command_line = format!(
            "ffmpeg {}",
            ffmpeg_args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );

        match self.start_ffmpeg_job(command_line.clone(), ffmpeg_args, output_path.clone()) {
            Ok(()) => {
                self.status_message = format!("Running ffmpeg -> {}", output_path.display());
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

    fn start_ffmpeg_job(
        &mut self,
        command_line: String,
        ffmpeg_args: Vec<String>,
        output_path: PathBuf,
    ) -> io::Result<()> {
        let mut child = Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("failed to capture ffmpeg stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("failed to capture ffmpeg stderr"))?;

        let (tx, rx) = mpsc::channel();
        spawn_ffmpeg_reader(stdout, FfmpegStream::Stdout, tx.clone());
        spawn_ffmpeg_reader(stderr, FfmpegStream::Stderr, tx);

        self.ffmpeg_spinner_frame = 0;
        self.ffmpeg_output = vec![
            format!("$ {command_line}"),
            "Streaming ffmpeg output...".to_string(),
        ];
        self.ffmpeg_scroll = self.ffmpeg_output.len().saturating_sub(1);
        self.running_trim = Some(RunningTrim {
            child,
            rx,
            command_line,
            output_path,
            stdout_raw: Vec::new(),
            stderr_raw: Vec::new(),
            stdout_pending: Vec::new(),
            stderr_pending: Vec::new(),
        });

        Ok(())
    }

    fn pump_running_trim_events(&mut self) {
        let mut streamed_lines = Vec::new();

        if let Some(running) = self.running_trim.as_mut() {
            while let Ok(event) = running.rx.try_recv() {
                match event {
                    FfmpegEvent::Chunk { stream, data } => {
                        let lines = consume_stream_chunk(running, stream, &data);
                        for line in lines {
                            streamed_lines.push((stream, line));
                        }
                    }
                    FfmpegEvent::ReaderError { stream, error } => {
                        streamed_lines.push((stream, format!("reader error: {error}")));
                    }
                }
            }
        }

        for (stream, line) in streamed_lines {
            self.append_stream_line(stream, line);
        }
    }

    fn try_finish_running_trim(&mut self) {
        let Some(status_result) = self
            .running_trim
            .as_mut()
            .map(|running| running.child.try_wait())
        else {
            return;
        };

        match status_result {
            Ok(Some(status)) => self.finish_running_trim(status),
            Ok(None) => {}
            Err(err) => {
                self.append_ffmpeg_output_line(format!("stderr: failed to poll ffmpeg: {err}"));
                self.status_message = format!("Failed to monitor ffmpeg process: {err}");
                self.running_trim = None;
            }
        }
    }

    fn finish_running_trim(&mut self, status: ExitStatus) {
        let Some(mut running) = self.running_trim.take() else {
            return;
        };

        while let Ok(event) = running.rx.try_recv() {
            match event {
                FfmpegEvent::Chunk { stream, data } => {
                    for line in consume_stream_chunk(&mut running, stream, &data) {
                        self.append_stream_line(stream, line);
                    }
                }
                FfmpegEvent::ReaderError { stream, error } => {
                    self.append_stream_line(stream, format!("reader error: {error}"));
                }
            }
        }

        if let Some(line) = flush_pending_line(&mut running.stderr_pending) {
            self.append_stream_line(FfmpegStream::Stderr, line);
        }
        if let Some(line) = flush_pending_line(&mut running.stdout_pending) {
            self.append_stream_line(FfmpegStream::Stdout, line);
        }

        let stdout_raw = running.stdout_raw;
        let stderr_raw = running.stderr_raw;
        let command_line = running.command_line;
        let output_path = running.output_path;

        if status.success() {
            let mut status_message = match self.append_ffmpeg_run_log(
                &command_line,
                status.code(),
                &stdout_raw,
                &stderr_raw,
                None,
            ) {
                Ok(log_path) => {
                    format!(
                        "Created clip: {} (log: {})",
                        output_path.display(),
                        log_path.display()
                    )
                }
                Err(log_err) => {
                    format!(
                        "Created clip: {} (log write failed: {log_err})",
                        output_path.display()
                    )
                }
            };

            if let Err(refresh_err) = self.refresh_file_browser_after_save(&output_path) {
                status_message.push_str(&format!(" (browser refresh failed: {refresh_err})"));
            }

            self.status_message = status_message;
        } else {
            let stderr = String::from_utf8_lossy(&stderr_raw);
            let detail = summarize_ffmpeg_error(&stderr);

            match self.append_ffmpeg_run_log(
                &command_line,
                status.code(),
                &stdout_raw,
                &stderr_raw,
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
    }

    fn append_stream_line(&mut self, stream: FfmpegStream, line: String) {
        let prefix = match stream {
            FfmpegStream::Stdout => "stdout",
            FfmpegStream::Stderr => "stderr",
        };
        self.append_ffmpeg_output_line(format!("{prefix}: {line}"));
    }

    fn append_ffmpeg_output_line(&mut self, line: String) {
        let follow_tail = self.ffmpeg_scroll >= self.ffmpeg_output.len().saturating_sub(1);
        self.ffmpeg_output.push(line);
        if follow_tail {
            self.ffmpeg_scroll = self.ffmpeg_output.len().saturating_sub(1);
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

    fn refresh_file_browser_after_save(&mut self, output_path: &Path) -> io::Result<()> {
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
        self.remove_audio = false;
        self.sync_output_name_to_available_for_path(&path);
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
        self.start_part = 0;
        self.end_part = 0;
        self.output_cursor = self.output_name.chars().count();
        self.selected_video = Some(path);
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

    fn sync_output_name_to_available_for_path(&mut self, input_path: &Path) {
        let requested_output_name = enforce_output_extension(&self.output_name, self.output_format);
        let requested_output_path = resolve_output_path(input_path, &requested_output_name);
        let normalized_output_path = output_path_without_numbered_suffix(&requested_output_path);
        let available_output_path = next_available_output_path(&normalized_output_path);
        self.sync_output_name_with_path(&requested_output_name, &available_output_path);
    }

    fn sync_output_name_with_path(&mut self, requested_output_name: &str, resolved_path: &Path) {
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

fn spinner_frames() -> &'static [char] {
    &['|', '/', '-', '\\']
}

fn spawn_ffmpeg_reader<R>(reader: R, stream: FfmpegStream, tx: mpsc::Sender<FfmpegEvent>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = [0_u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    if tx
                        .send(FfmpegEvent::Chunk {
                            stream,
                            data: buf[..read].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(FfmpegEvent::ReaderError {
                        stream,
                        error: err.to_string(),
                    });
                    break;
                }
            }
        }
    });
}

fn consume_stream_chunk(
    running: &mut RunningTrim,
    stream: FfmpegStream,
    data: &[u8],
) -> Vec<String> {
    let (raw, pending) = match stream {
        FfmpegStream::Stdout => (&mut running.stdout_raw, &mut running.stdout_pending),
        FfmpegStream::Stderr => (&mut running.stderr_raw, &mut running.stderr_pending),
    };

    raw.extend_from_slice(data);

    let mut lines = Vec::new();
    for &byte in data {
        if byte == b'\n' || byte == b'\r' {
            if let Some(line) = flush_pending_line(pending) {
                lines.push(line);
            }
        } else {
            pending.push(byte);
        }
    }

    lines
}

fn flush_pending_line(pending: &mut Vec<u8>) -> Option<String> {
    if pending.is_empty() {
        return None;
    }

    let line = String::from_utf8_lossy(pending)
        .trim_end_matches(['\n', '\r'])
        .to_string();
    pending.clear();

    if line.is_empty() { None } else { Some(line) }
}
