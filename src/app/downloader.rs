// Downloader tab runtime behavior.
// - Owns the editable URL field for the downloader tab.
// - Spawns yt-dlp in the background and streams stdout/stderr incrementally.
// - Keeps a scrollable in-memory output buffer for the reusable log panel.
// - Refreshes the file browser after successful downloads so new files appear immediately.
use std::{
    io::{self, BufReader, Read},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
};

use crate::media::shell_quote;

use super::{App, RunningDownloader, DownloaderEvent, DownloaderStream};

impl App {
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
            self.status_message = "Downloader is already running. Wait for it to finish.".to_string();
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

        let downloader_args = vec![
            "--newline".to_string(),
            "-P".to_string(),
            self.cwd.display().to_string(),
            url,
        ];

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
                self.status_message = format!("Running Downloader -> {}", self.cwd.display());
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

    pub fn move_downloader_cursor_left(&mut self) {
        self.downloader_url_cursor = self.downloader_url_cursor.saturating_sub(1);
    }

    pub fn move_downloader_cursor_right(&mut self) {
        let max = self.downloader_url.chars().count();
        self.downloader_url_cursor = (self.downloader_url_cursor + 1).min(max);
    }

    pub fn backspace_downloader_url(&mut self) {
        if self.downloader_url_cursor == 0 {
            return;
        }

        let remove_char_index = self.downloader_url_cursor - 1;
        let start = super::input::byte_index_for_char(&self.downloader_url, remove_char_index);
        let end = super::input::byte_index_for_char(&self.downloader_url, remove_char_index + 1);
        self.downloader_url.replace_range(start..end, "");
        self.downloader_url_cursor -= 1;
    }

    pub fn push_downloader_url_char(&mut self, ch: char) {
        let byte_index =
            super::input::byte_index_for_char(&self.downloader_url, self.downloader_url_cursor);
        self.downloader_url.insert(byte_index, ch);
        self.downloader_url_cursor += 1;
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

fn spawn_downloader_reader<R>(reader: R, stream: DownloaderStream, tx: mpsc::Sender<DownloaderEvent>)
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
