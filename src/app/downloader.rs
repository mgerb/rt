// Downloader tab runtime behavior.
// - Owns the 2-step downloader flow:
//   Step 1: edit URL and press Enter to fetch available quality options.
//   Step 2: choose quality and press Enter to start yt-dlp download.
// - Runs both metadata probing and downloads without blocking the UI event loop.
// - Streams yt-dlp stdout/stderr incrementally into the shared tool output panel.
// - Refreshes the file browser after successful downloads so new files appear immediately.
use std::{
    cmp::Ordering,
    collections::HashSet,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, TryRecvError},
    thread,
};

use crate::{
    media::{next_available_output_path, shell_quote},
    model::DownloaderStep,
};

use super::{
    App, DownloaderEvent, DownloaderProbeResult, DownloaderQualityChoice, DownloaderStream,
    RunningDownloader, RunningDownloaderProbe,
};

const QUALITY_ID_WIDTH: usize = 7;
const QUALITY_EXT_WIDTH: usize = 4;
const QUALITY_RES_WIDTH: usize = 9;
const QUALITY_FPS_WIDTH: usize = 6;
const QUALITY_SIZE_WIDTH: usize = 10;
const QUALITY_AUD_WIDTH: usize = 5;
const DOWNLOADER_OPTION_COUNT: usize = 3;

impl App {
    pub fn downloader_step(&self) -> DownloaderStep {
        self.downloader_step
    }

    pub fn downloader_quality_position(&self) -> (usize, usize) {
        let total = self.downloader_quality_choices.len();
        if total == 0 {
            return (0, 0);
        }
        let selected = self.downloader_quality_index.min(total - 1) + 1;
        (selected, total)
    }

    pub fn downloader_selected_quality_selector(&self) -> String {
        self.effective_downloader_selector(&self.selected_downloader_quality().selector)
    }

    pub fn downloader_quality_header_row(&self) -> String {
        format_quality_columns("ID", "EXT", "RES", "FPS", "SIZE", "AUDIO", "TYPE")
    }

    pub fn downloader_audio_only_enabled(&self) -> bool {
        self.downloader_audio_only
    }

    pub fn downloader_sponsorblock_enabled(&self) -> bool {
        self.downloader_sponsorblock
    }

    pub fn downloader_subtitles_enabled(&self) -> bool {
        self.downloader_subtitles
    }

    pub fn downloader_option_focus_index(&self) -> Option<usize> {
        self.downloader_option_focus
            .map(|index| index.min(DOWNLOADER_OPTION_COUNT.saturating_sub(1)))
    }

    pub fn downloader_quality_list_focused(&self) -> bool {
        self.downloader_option_focus.is_none()
    }

    pub fn downloader_visible_quality_rows(&self, max_visible: usize) -> (Vec<String>, usize) {
        if self.downloader_quality_choices.is_empty() {
            return (vec![], 0);
        }

        let max_visible = max_visible.max(1);
        let total = self.downloader_quality_choices.len();
        let selected = self.downloader_quality_index.min(total - 1);
        let half = max_visible / 2;
        let mut start = selected.saturating_sub(half);
        let mut end = (start + max_visible).min(total);
        if end - start < max_visible {
            start = end.saturating_sub(max_visible);
            end = (start + max_visible).min(total);
        }

        let rows = self.downloader_quality_choices[start..end]
            .iter()
            .map(|choice| choice.label.clone())
            .collect::<Vec<_>>();

        (rows, selected.saturating_sub(start))
    }

    pub fn downloader_is_fetching_qualities(&self) -> bool {
        self.running_downloader_probe.is_some()
    }

    pub fn downloader_accepts_text_input(&self) -> bool {
        self.downloader_step == DownloaderStep::UrlInput && self.running_downloader_probe.is_none()
    }

    pub fn downloader_press_enter(&mut self) {
        if self.running_downloader.is_some() {
            self.status_message =
                "Downloader is already running. Wait for it to finish.".to_string();
            return;
        }
        if self.running_downloader_probe.is_some() {
            self.status_message = "Still fetching quality options. Please wait.".to_string();
            return;
        }

        match self.downloader_step {
            DownloaderStep::UrlInput => self.fetch_downloader_qualities(),
            DownloaderStep::QualitySelect => {
                if self.downloader_quality_list_focused() {
                    self.run_downloader_download();
                } else {
                    self.toggle_focused_downloader_option();
                }
            }
        }
    }

    pub fn move_downloader_cursor_left(&mut self) {
        match self.downloader_step {
            DownloaderStep::UrlInput => {
                self.downloader_url_cursor = self.downloader_url_cursor.saturating_sub(1);
            }
            DownloaderStep::QualitySelect => {}
        }
    }

    pub fn move_downloader_cursor_right(&mut self) {
        match self.downloader_step {
            DownloaderStep::UrlInput => {
                let max = self.downloader_url.chars().count();
                self.downloader_url_cursor = (self.downloader_url_cursor + 1).min(max);
            }
            DownloaderStep::QualitySelect => {}
        }
    }

    pub fn backspace_downloader_url(&mut self) {
        match self.downloader_step {
            DownloaderStep::UrlInput => {
                if self.downloader_url_cursor == 0 {
                    return;
                }

                let remove_char_index = self.downloader_url_cursor - 1;
                let start =
                    super::input::byte_index_for_char(&self.downloader_url, remove_char_index);
                let end =
                    super::input::byte_index_for_char(&self.downloader_url, remove_char_index + 1);
                self.downloader_url.replace_range(start..end, "");
                self.downloader_url_cursor -= 1;
            }
            DownloaderStep::QualitySelect => {
                self.downloader_step = DownloaderStep::UrlInput;
                self.downloader_option_focus = None;
                self.status_message =
                    "Returned to URL edit step. Press Enter to refresh qualities.".to_string();
            }
        }
    }

    pub fn push_downloader_url_char(&mut self, ch: char) {
        match self.downloader_step {
            DownloaderStep::UrlInput => {
                let byte_index = super::input::byte_index_for_char(
                    &self.downloader_url,
                    self.downloader_url_cursor,
                );
                self.downloader_url.insert(byte_index, ch);
                self.downloader_url_cursor += 1;
            }
            DownloaderStep::QualitySelect => match ch {
                'j' => {
                    if self.downloader_quality_list_focused() {
                        self.select_next_downloader_quality();
                    }
                }
                'k' => {
                    if self.downloader_quality_list_focused() {
                        self.select_previous_downloader_quality();
                    }
                }
                _ => {}
            },
        }
    }

    pub fn select_downloader_quality_up(&mut self) {
        if self.downloader_step == DownloaderStep::QualitySelect
            && self.downloader_quality_list_focused()
        {
            self.select_previous_downloader_quality();
        }
    }

    pub fn select_downloader_quality_down(&mut self) {
        if self.downloader_step == DownloaderStep::QualitySelect
            && self.downloader_quality_list_focused()
        {
            self.select_next_downloader_quality();
        }
    }

    pub fn next_downloader_option_focus(&mut self) {
        if self.downloader_step != DownloaderStep::QualitySelect {
            return;
        }
        self.downloader_option_focus = match self.downloader_option_focus {
            None => Some(0),
            Some(index) if index + 1 < DOWNLOADER_OPTION_COUNT => Some(index + 1),
            Some(_) => {
                self.downloader_quality_index = 0;
                None
            }
        };
    }

    pub fn previous_downloader_option_focus(&mut self) {
        if self.downloader_step != DownloaderStep::QualitySelect {
            return;
        }
        self.downloader_option_focus = match self.downloader_option_focus {
            None => Some(DOWNLOADER_OPTION_COUNT - 1),
            Some(0) => {
                self.downloader_quality_index = 0;
                None
            }
            Some(index) => Some(index - 1),
        };
    }

    pub fn toggle_focused_downloader_option(&mut self) {
        if self.downloader_step != DownloaderStep::QualitySelect {
            return;
        }

        match self.downloader_option_focus_index() {
            Some(0) => self.toggle_downloader_audio_only(),
            Some(1) => self.toggle_downloader_sponsorblock(),
            Some(2) => self.toggle_downloader_subtitles(),
            _ => {}
        }
    }

    pub fn scroll_downloader_output_down(&mut self) {
        self.downloader_output.scroll_down();
    }

    pub fn scroll_downloader_output_up(&mut self) {
        self.downloader_output.scroll_up();
    }

    pub fn page_downloader_output_down(&mut self) {
        self.downloader_output.page_down();
    }

    pub fn page_downloader_output_up(&mut self) {
        self.downloader_output.page_up();
    }

    pub fn cancel_downloader(&mut self) {
        let Some(running) = self.running_downloader.as_mut() else {
            self.status_message = "No running downloader job to cancel.".to_string();
            return;
        };

        match running.child.try_wait() {
            Ok(Some(_)) => {
                self.status_message = "Downloader job is already finishing.".to_string();
            }
            Ok(None) => match running.child.kill() {
                Ok(()) => {
                    self.status_message = "Cancellation requested for downloader job.".to_string();
                    self.downloader_output
                        .append_line("Cancellation requested by user (x).".to_string());
                }
                Err(err) => {
                    self.status_message = format!("Failed to cancel downloader job: {err}");
                }
            },
            Err(err) => {
                self.status_message = format!("Failed to inspect downloader process: {err}");
            }
        }
    }

    pub fn run_downloader_download(&mut self) {
        if self.running_downloader.is_some() {
            self.status_message =
                "Downloader is already running. Wait for it to finish.".to_string();
            return;
        }
        if self.running_downloader_probe.is_some() {
            self.status_message = "Still fetching quality options. Please wait.".to_string();
            return;
        }
        if !self.downloader_available() {
            self.status_message =
                "Downloader requires yt-dlp in PATH. Install it to enable downloads.".to_string();
            return;
        }

        let url = self.downloader_url.trim().to_string();
        if url.is_empty() {
            self.status_message = "Enter a URL before running Downloader.".to_string();
            return;
        }

        let selected_quality = self.selected_downloader_quality();
        let effective_selector = self.effective_downloader_selector(&selected_quality.selector);
        let output_path = match resolve_downloader_output_path(
            &self.cwd,
            &url,
            &effective_selector,
            self.downloader_audio_only,
            self.downloader_subtitles,
        ) {
            Ok(path) => path,
            Err(err) => {
                self.status_message = format!("Failed to resolve downloader output name: {err}");
                return;
            }
        };

        let mut downloader_args = vec![
            "--newline".to_string(),
            "--no-overwrites".to_string(),
            "-f".to_string(),
            effective_selector.clone(),
            "-o".to_string(),
            output_path.display().to_string(),
        ];
        if self.downloader_audio_only {
            downloader_args.extend([
                "-x".to_string(),
                "--audio-format".to_string(),
                "mp3".to_string(),
            ]);
        }
        if self.downloader_sponsorblock {
            downloader_args.extend(["--sponsorblock-remove".to_string(), "default".to_string()]);
        }
        if self.downloader_subtitles {
            downloader_args.extend([
                "--write-subs".to_string(),
                "--write-auto-subs".to_string(),
                "--sub-langs".to_string(),
                "all,-live_chat".to_string(),
            ]);
        }
        downloader_args.push(url);

        let command_line = format!(
            "yt-dlp {}",
            downloader_args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );

        match self.start_downloader_job(command_line.clone(), downloader_args) {
            Ok(()) => {
                self.status_message = format!(
                    "Running Downloader ({}) -> {}",
                    self.downloader_run_mode_label(&selected_quality.label),
                    output_path.display()
                );
            }
            Err(err) => {
                self.downloader_output.replace_with_command_error(
                    &command_line,
                    &format!("Failed to start Downloader: {err}"),
                );
                self.status_message = format!("Failed to start Downloader: {err}");
            }
        }
    }

    pub(super) fn try_finish_running_downloader_probe(&mut self) {
        let probe_result = {
            let Some(running) = self.running_downloader_probe.as_mut() else {
                return;
            };

            match running.rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(DownloaderProbeResult::Failed {
                    error: "Failed to receive quality options from probe thread.".to_string(),
                }),
            }
        };

        let Some(result) = probe_result else {
            return;
        };

        let command_line = self
            .running_downloader_probe
            .take()
            .map(|running| running.command_line)
            .unwrap_or_else(|| "yt-dlp -F".to_string());

        match result {
            DownloaderProbeResult::Success { choices } => {
                self.downloader_quality_choices = choices;
                self.downloader_quality_index = 0;
                self.downloader_option_focus = Some(0);
                self.downloader_step = DownloaderStep::QualitySelect;

                let (_, total) = self.downloader_quality_position();
                self.status_message = format!(
                    "Loaded {total} video quality options. Use Up/Down (or j/k), then Enter to download."
                );
                self.downloader_output
                    .begin_stream(&command_line, "Video quality options loaded.");
                self.downloader_output
                    .append_line(format!("Detected {total} video quality options."));
            }
            DownloaderProbeResult::Failed { error } => {
                self.downloader_step = DownloaderStep::UrlInput;
                self.downloader_output
                    .replace_with_command_error(&command_line, &error);
                self.status_message = error;
            }
        }
    }

    fn fetch_downloader_qualities(&mut self) {
        if !self.downloader_available() {
            self.status_message =
                "Downloader requires yt-dlp in PATH. Install it to enable downloads.".to_string();
            return;
        }

        let url = self.downloader_url.trim().to_string();
        if url.is_empty() {
            self.status_message = "Enter a URL before fetching quality options.".to_string();
            return;
        }

        let command_line = format!("yt-dlp -F {}", shell_quote(&url));
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = probe_downloader_qualities(&url);
            let _ = tx.send(result);
        });

        self.running_downloader_probe = Some(RunningDownloaderProbe {
            rx,
            command_line: command_line.clone(),
        });
        self.downloader_spinner_frame = 0;
        self.downloader_output
            .begin_stream(&command_line, "Fetching quality options...");
        self.status_message = "Fetching available downloader qualities...".to_string();
    }

    fn toggle_downloader_audio_only(&mut self) {
        self.downloader_audio_only = !self.downloader_audio_only;
        self.status_message = format!(
            "Downloader option: audio-only {}.",
            on_off(self.downloader_audio_only)
        );
    }

    fn toggle_downloader_sponsorblock(&mut self) {
        self.downloader_sponsorblock = !self.downloader_sponsorblock;
        self.status_message = format!(
            "Downloader option: SponsorBlock {}.",
            on_off(self.downloader_sponsorblock)
        );
    }

    fn toggle_downloader_subtitles(&mut self) {
        self.downloader_subtitles = !self.downloader_subtitles;
        self.status_message = format!(
            "Downloader option: subtitles {}.",
            on_off(self.downloader_subtitles)
        );
    }

    fn effective_downloader_selector(&self, selected_selector: &str) -> String {
        if self.downloader_audio_only {
            "bestaudio/best".to_string()
        } else {
            selected_selector.to_string()
        }
    }

    fn downloader_run_mode_label(&self, quality_label: &str) -> String {
        let mut flags = Vec::new();
        if self.downloader_audio_only {
            flags.push("audio-only");
        }
        if self.downloader_sponsorblock {
            flags.push("sponsorblock");
        }
        if self.downloader_subtitles {
            flags.push("subtitles");
        }

        if flags.is_empty() {
            quality_label.to_string()
        } else {
            format!("{quality_label}; {}", flags.join(", "))
        }
    }

    fn start_downloader_job(
        &mut self,
        command_line: String,
        downloader_args: Vec<String>,
    ) -> io::Result<()> {
        let mut child = Command::new("yt-dlp")
            .args(&downloader_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("failed to capture yt-dlp stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("failed to capture yt-dlp stderr"))?;

        let (tx, rx) = mpsc::channel();
        spawn_downloader_reader(stdout, DownloaderStream::Stdout, tx.clone());
        spawn_downloader_reader(stderr, DownloaderStream::Stderr, tx);

        self.downloader_spinner_frame = 0;
        self.downloader_output
            .begin_stream(&command_line, "Streaming yt-dlp output...");
        self.running_downloader = Some(RunningDownloader {
            child,
            rx,
            command_line,
            stdout_raw: Vec::new(),
            stderr_raw: Vec::new(),
            stdout_pending: Vec::new(),
            stderr_pending: Vec::new(),
        });

        Ok(())
    }

    fn select_previous_downloader_quality(&mut self) {
        let total = self.downloader_quality_choices.len();
        if total <= 1 {
            return;
        }

        self.downloader_quality_index = if self.downloader_quality_index == 0 {
            total - 1
        } else {
            self.downloader_quality_index - 1
        };
    }

    fn select_next_downloader_quality(&mut self) {
        let total = self.downloader_quality_choices.len();
        if total <= 1 {
            return;
        }

        self.downloader_quality_index = (self.downloader_quality_index + 1) % total;
    }

    fn selected_downloader_quality(&self) -> DownloaderQualityChoice {
        self.downloader_quality_choices
            .get(self.downloader_quality_index)
            .cloned()
            .unwrap_or_else(default_downloader_quality_choice)
    }

    pub(super) fn pump_running_downloader_events(&mut self) {
        let mut streamed_lines = Vec::new();

        if let Some(running) = self.running_downloader.as_mut() {
            while let Ok(event) = running.rx.try_recv() {
                match event {
                    DownloaderEvent::Chunk { stream, data } => {
                        let lines = consume_stream_chunk(running, stream, &data);
                        for line in lines {
                            streamed_lines.push((stream, line));
                        }
                    }
                    DownloaderEvent::ReaderError { stream, error } => {
                        streamed_lines.push((stream, format!("reader error: {error}")));
                    }
                }
            }
        }

        for (stream, line) in streamed_lines {
            self.append_downloader_stream_line(stream, line);
        }
    }

    pub(super) fn try_finish_running_downloader(&mut self) {
        let Some(status_result) = self
            .running_downloader
            .as_mut()
            .map(|running| running.child.try_wait())
        else {
            return;
        };

        match status_result {
            Ok(Some(status)) => self.finish_running_downloader(status),
            Ok(None) => {}
            Err(err) => {
                self.append_downloader_output_line(format!(
                    "stderr: failed to poll Downloader process: {err}"
                ));
                self.status_message = format!("Failed to monitor Downloader process: {err}");
                self.running_downloader = None;
            }
        }
    }

    fn finish_running_downloader(&mut self, status: ExitStatus) {
        let Some(mut running) = self.running_downloader.take() else {
            return;
        };

        while let Ok(event) = running.rx.try_recv() {
            match event {
                DownloaderEvent::Chunk { stream, data } => {
                    for line in consume_stream_chunk(&mut running, stream, &data) {
                        self.append_downloader_stream_line(stream, line);
                    }
                }
                DownloaderEvent::ReaderError { stream, error } => {
                    self.append_downloader_stream_line(stream, format!("reader error: {error}"));
                }
            }
        }

        if let Some(line) = flush_pending_line(&mut running.stderr_pending) {
            self.append_downloader_stream_line(DownloaderStream::Stderr, line);
        }
        if let Some(line) = flush_pending_line(&mut running.stdout_pending) {
            self.append_downloader_stream_line(DownloaderStream::Stdout, line);
        }

        let command_line = running.command_line;
        let _stdout_raw = running.stdout_raw;
        let stderr_raw = running.stderr_raw;

        if status.success() {
            if let Err(err) = self.reload() {
                self.status_message =
                    format!("Downloader completed, but browser refresh failed: {err}");
            } else {
                self.status_message = "Downloader completed successfully.".to_string();
            }
        } else {
            let stderr = String::from_utf8_lossy(&stderr_raw);
            let detail = stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .next_back()
                .unwrap_or("unknown yt-dlp error");
            self.status_message = format!("Downloader failed: {detail}");
        }

        self.append_downloader_output_line(format!(
            "Downloader finished with exit code: {} ({command_line})",
            status.code().unwrap_or(-1)
        ));
    }

    fn append_downloader_stream_line(&mut self, stream: DownloaderStream, line: String) {
        let prefix = match stream {
            DownloaderStream::Stdout => "stdout",
            DownloaderStream::Stderr => "stderr",
        };
        self.downloader_output.append_prefixed(prefix, line);
    }

    fn append_downloader_output_line(&mut self, line: String) {
        self.downloader_output.append_line(line);
    }
}

fn default_downloader_quality_choice() -> DownloaderQualityChoice {
    DownloaderQualityChoice {
        selector: "bestvideo+bestaudio/best".to_string(),
        label: format_quality_columns("AUTO", "auto", "best", "--", "--", "auto", "video"),
    }
}

fn probe_downloader_qualities(url: &str) -> DownloaderProbeResult {
    let output = match Command::new("yt-dlp").args(["-F", url]).output() {
        Ok(output) => output,
        Err(err) => {
            return DownloaderProbeResult::Failed {
                error: format!("Failed to execute yt-dlp format probe: {err}"),
            };
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .next_back()
            .unwrap_or("yt-dlp failed to fetch formats");
        return DownloaderProbeResult::Failed {
            error: format!("Format probe failed: {detail}"),
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut choices = parse_quality_choices_from_format_list(&stdout);
    if choices.is_empty() {
        choices.push(default_downloader_quality_choice());
    }

    DownloaderProbeResult::Success { choices }
}

fn parse_quality_choices_from_format_list(output: &str) -> Vec<DownloaderQualityChoice> {
    let mut seen = HashSet::new();
    seen.insert("bestvideo+bestaudio/best".to_string());
    let mut candidates = Vec::new();

    let mut in_table = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !in_table {
            if trimmed.contains("ID") && trimmed.contains("EXT") {
                in_table = true;
            }
            continue;
        }

        if trimmed.starts_with('-') {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let Some(format_id) = parts.next() else {
            continue;
        };

        if !is_format_id_token(format_id) {
            continue;
        }
        if trimmed.contains("audio only")
            || trimmed.contains("images")
            || trimmed.contains("storyboard")
        {
            continue;
        }

        let Some(ext) = parts.next() else {
            continue;
        };
        let Some(resolution) = extract_resolution_token(trimmed) else {
            continue;
        };

        let fps = extract_fps_token(trimmed).unwrap_or_else(|| "--".to_string());
        let size = extract_size_token(trimmed).unwrap_or_else(|| "--".to_string());
        let size_bytes = parse_size_bytes(&size);
        let video_only = trimmed.contains("video only");
        let has_audio = !video_only;
        let selector = if video_only {
            format!("{format_id}+bestaudio/best")
        } else {
            format_id.to_string()
        };
        if !seen.insert(selector.clone()) {
            continue;
        }

        candidates.push(QualityCandidate {
            choice: DownloaderQualityChoice {
                selector,
                label: format_quality_columns(
                    format_id,
                    ext,
                    &resolution,
                    &fps,
                    &size,
                    if has_audio { "yes" } else { "no" },
                    if video_only { "video" } else { "muxed" },
                ),
            },
            size_bytes,
            original_index: candidates.len(),
        });

        if candidates.len() >= 79 {
            break;
        }
    }

    candidates.sort_by(compare_quality_candidates);

    let mut choices = vec![default_downloader_quality_choice()];
    choices.extend(candidates.into_iter().map(|entry| entry.choice));
    choices
}

fn format_quality_columns(
    id: &str,
    ext: &str,
    resolution: &str,
    fps: &str,
    size: &str,
    audio: &str,
    kind: &str,
) -> String {
    format!(
        "{:<QUALITY_ID_WIDTH$} {:<QUALITY_EXT_WIDTH$} {:<QUALITY_RES_WIDTH$} {:<QUALITY_FPS_WIDTH$} {:<QUALITY_SIZE_WIDTH$} {:<QUALITY_AUD_WIDTH$} {}",
        id, ext, resolution, fps, size, audio, kind
    )
}

fn extract_resolution_token(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|token| {
        let clean = token.trim_matches(|ch: char| matches!(ch, ',' | '[' | ']'));
        let (left, right) = clean.split_once('x')?;
        if left.is_empty() || right.is_empty() {
            return None;
        }
        if !left.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        if !right.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        Some(clean.to_string())
    })
}

fn extract_fps_token(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|token| {
        let clean = token.trim_end_matches(',');
        if let Some(prefix) = clean.strip_suffix("fps")
            && !prefix.is_empty()
            && prefix.chars().all(|ch| ch.is_ascii_digit())
        {
            return Some(clean.to_string());
        }
        None
    })
}

fn extract_size_token(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|token| {
        let clean = token.trim_matches(|ch: char| matches!(ch, ',' | '[' | ']'));
        let normalized = clean.strip_prefix('~').unwrap_or(clean);
        if normalized.len() < 3 {
            return None;
        }

        let mut chars = normalized.chars().peekable();
        let mut saw_digit = false;
        while let Some(ch) = chars.peek().copied() {
            if ch.is_ascii_digit() || ch == '.' {
                saw_digit = true;
                chars.next();
            } else {
                break;
            }
        }
        if !saw_digit {
            return None;
        }

        let unit = chars.collect::<String>();
        match unit.as_str() {
            "B" | "KiB" | "MiB" | "GiB" | "TiB" => Some(clean.to_string()),
            _ => None,
        }
    })
}

fn parse_size_bytes(size_token: &str) -> Option<u64> {
    let normalized = size_token.trim().trim_start_matches('~');
    if normalized.is_empty() || normalized == "--" {
        return None;
    }

    let split_at = normalized.find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))?;
    let (number, unit) = normalized.split_at(split_at);
    let value = number.parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let multiplier = match unit {
        "B" => 1_f64,
        "KiB" => 1024_f64,
        "MiB" => 1024_f64.powi(2),
        "GiB" => 1024_f64.powi(3),
        "TiB" => 1024_f64.powi(4),
        _ => return None,
    };

    Some((value * multiplier).round() as u64)
}

#[derive(Debug)]
struct QualityCandidate {
    choice: DownloaderQualityChoice,
    size_bytes: Option<u64>,
    original_index: usize,
}

fn compare_quality_candidates(left: &QualityCandidate, right: &QualityCandidate) -> Ordering {
    match (left.size_bytes, right.size_bytes) {
        (Some(a), Some(b)) => a
            .cmp(&b)
            .then_with(|| left.original_index.cmp(&right.original_index)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.original_index.cmp(&right.original_index),
    }
}

fn is_format_id_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }

    token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn spawn_downloader_reader<R>(
    reader: R,
    stream: DownloaderStream,
    tx: mpsc::Sender<DownloaderEvent>,
) where
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
                        .send(DownloaderEvent::Chunk {
                            stream,
                            data: buf[..read].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(DownloaderEvent::ReaderError {
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
    running: &mut RunningDownloader,
    stream: DownloaderStream,
    data: &[u8],
) -> Vec<String> {
    let (raw, pending) = match stream {
        DownloaderStream::Stdout => (&mut running.stdout_raw, &mut running.stdout_pending),
        DownloaderStream::Stderr => (&mut running.stderr_raw, &mut running.stderr_pending),
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

fn resolve_downloader_output_path(
    cwd: &Path,
    url: &str,
    selector: &str,
    audio_only: bool,
    subtitles: bool,
) -> io::Result<PathBuf> {
    let mut probe_args = vec![
        "--print".to_string(),
        "filename".to_string(),
        "--skip-download".to_string(),
        "--no-warnings".to_string(),
        "--no-playlist".to_string(),
        "-f".to_string(),
        selector.to_string(),
        "-P".to_string(),
        cwd.display().to_string(),
        "-o".to_string(),
        "%(title)s.%(ext)s".to_string(),
    ];
    if audio_only {
        probe_args.extend([
            "-x".to_string(),
            "--audio-format".to_string(),
            "mp3".to_string(),
        ]);
    }
    if subtitles {
        probe_args.extend([
            "--write-subs".to_string(),
            "--write-auto-subs".to_string(),
            "--sub-langs".to_string(),
            "all,-live_chat".to_string(),
        ]);
    }
    probe_args.push(url.to_string());

    let probe_output = Command::new("yt-dlp").args(&probe_args).output()?;
    if !probe_output.status.success() {
        let stderr = String::from_utf8_lossy(&probe_output.stderr);
        let detail = stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .next_back()
            .unwrap_or("yt-dlp failed to compute output filename");
        return Err(io::Error::other(detail.to_string()));
    }

    let stdout = String::from_utf8_lossy(&probe_output.stdout);
    let Some(filename_line) = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .next_back()
    else {
        return Err(io::Error::other(
            "yt-dlp did not return a predicted output filename",
        ));
    };

    let predicted_path = PathBuf::from(filename_line);
    let absolute_predicted = if predicted_path.is_absolute() {
        predicted_path
    } else {
        cwd.join(predicted_path)
    };

    Ok(next_available_output_path(&absolute_predicted))
}

fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}
