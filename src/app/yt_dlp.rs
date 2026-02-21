// yt-dlp tab runtime behavior.
// - Owns the editable URL field for the yt-dlp tab.
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

use super::{App, RunningYtDlp, YtDlpEvent, YtDlpStream};

impl App {
    pub fn run_yt_dlp_download(&mut self) {
        if self.running_yt_dlp.is_some() {
            self.status_message = "yt-dlp is already running. Wait for it to finish.".to_string();
            return;
        }
        if !self.yt_dlp_available() {
            self.status_message =
                "yt-dlp was not found in PATH. Install yt-dlp to enable downloads.".to_string();
            return;
        }

        let url = self.yt_dlp_url.trim().to_string();
        if url.is_empty() {
            self.status_message = "Enter a URL before running yt-dlp.".to_string();
            return;
        }

        let yt_dlp_args = vec![
            "--newline".to_string(),
            "-P".to_string(),
            self.cwd.display().to_string(),
            url,
        ];

        let command_line = format!(
            "yt-dlp {}",
            yt_dlp_args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );

        match self.start_yt_dlp_job(command_line.clone(), yt_dlp_args) {
            Ok(()) => {
                self.status_message = format!("Running yt-dlp -> {}", self.cwd.display());
            }
            Err(err) => {
                self.yt_dlp_output.replace_with_command_error(
                    &command_line,
                    &format!("Failed to start yt-dlp: {err}"),
                );
                self.status_message = format!("Failed to start yt-dlp: {err}");
            }
        }
    }

    pub fn move_yt_dlp_cursor_left(&mut self) {
        self.yt_dlp_url_cursor = self.yt_dlp_url_cursor.saturating_sub(1);
    }

    pub fn move_yt_dlp_cursor_right(&mut self) {
        let max = self.yt_dlp_url.chars().count();
        self.yt_dlp_url_cursor = (self.yt_dlp_url_cursor + 1).min(max);
    }

    pub fn backspace_yt_dlp_url(&mut self) {
        if self.yt_dlp_url_cursor == 0 {
            return;
        }

        let remove_char_index = self.yt_dlp_url_cursor - 1;
        let start = super::input::byte_index_for_char(&self.yt_dlp_url, remove_char_index);
        let end = super::input::byte_index_for_char(&self.yt_dlp_url, remove_char_index + 1);
        self.yt_dlp_url.replace_range(start..end, "");
        self.yt_dlp_url_cursor -= 1;
    }

    pub fn push_yt_dlp_url_char(&mut self, ch: char) {
        let byte_index =
            super::input::byte_index_for_char(&self.yt_dlp_url, self.yt_dlp_url_cursor);
        self.yt_dlp_url.insert(byte_index, ch);
        self.yt_dlp_url_cursor += 1;
    }

    pub fn scroll_yt_dlp_output_down(&mut self) {
        self.yt_dlp_output.scroll_down();
    }

    pub fn scroll_yt_dlp_output_up(&mut self) {
        self.yt_dlp_output.scroll_up();
    }

    pub fn page_yt_dlp_output_down(&mut self) {
        self.yt_dlp_output.page_down();
    }

    pub fn page_yt_dlp_output_up(&mut self) {
        self.yt_dlp_output.page_up();
    }

    fn start_yt_dlp_job(
        &mut self,
        command_line: String,
        yt_dlp_args: Vec<String>,
    ) -> io::Result<()> {
        let mut child = Command::new("yt-dlp")
            .args(&yt_dlp_args)
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
        spawn_yt_dlp_reader(stdout, YtDlpStream::Stdout, tx.clone());
        spawn_yt_dlp_reader(stderr, YtDlpStream::Stderr, tx);

        self.yt_dlp_spinner_frame = 0;
        self.yt_dlp_output
            .begin_stream(&command_line, "Streaming yt-dlp output...");
        self.running_yt_dlp = Some(RunningYtDlp {
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

    pub(super) fn pump_running_yt_dlp_events(&mut self) {
        let mut streamed_lines = Vec::new();

        if let Some(running) = self.running_yt_dlp.as_mut() {
            while let Ok(event) = running.rx.try_recv() {
                match event {
                    YtDlpEvent::Chunk { stream, data } => {
                        let lines = consume_stream_chunk(running, stream, &data);
                        for line in lines {
                            streamed_lines.push((stream, line));
                        }
                    }
                    YtDlpEvent::ReaderError { stream, error } => {
                        streamed_lines.push((stream, format!("reader error: {error}")));
                    }
                }
            }
        }

        for (stream, line) in streamed_lines {
            self.append_yt_dlp_stream_line(stream, line);
        }
    }

    pub(super) fn try_finish_running_yt_dlp(&mut self) {
        let Some(status_result) = self
            .running_yt_dlp
            .as_mut()
            .map(|running| running.child.try_wait())
        else {
            return;
        };

        match status_result {
            Ok(Some(status)) => self.finish_running_yt_dlp(status),
            Ok(None) => {}
            Err(err) => {
                self.append_yt_dlp_output_line(format!("stderr: failed to poll yt-dlp: {err}"));
                self.status_message = format!("Failed to monitor yt-dlp process: {err}");
                self.running_yt_dlp = None;
            }
        }
    }

    fn finish_running_yt_dlp(&mut self, status: ExitStatus) {
        let Some(mut running) = self.running_yt_dlp.take() else {
            return;
        };

        while let Ok(event) = running.rx.try_recv() {
            match event {
                YtDlpEvent::Chunk { stream, data } => {
                    for line in consume_stream_chunk(&mut running, stream, &data) {
                        self.append_yt_dlp_stream_line(stream, line);
                    }
                }
                YtDlpEvent::ReaderError { stream, error } => {
                    self.append_yt_dlp_stream_line(stream, format!("reader error: {error}"));
                }
            }
        }

        if let Some(line) = flush_pending_line(&mut running.stderr_pending) {
            self.append_yt_dlp_stream_line(YtDlpStream::Stderr, line);
        }
        if let Some(line) = flush_pending_line(&mut running.stdout_pending) {
            self.append_yt_dlp_stream_line(YtDlpStream::Stdout, line);
        }

        let command_line = running.command_line;
        let _stdout_raw = running.stdout_raw;
        let stderr_raw = running.stderr_raw;

        if status.success() {
            if let Err(err) = self.reload() {
                self.status_message =
                    format!("yt-dlp completed, but browser refresh failed: {err}");
            } else {
                self.status_message = "yt-dlp completed successfully.".to_string();
            }
        } else {
            let stderr = String::from_utf8_lossy(&stderr_raw);
            let detail = stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .next_back()
                .unwrap_or("unknown yt-dlp error");
            self.status_message = format!("yt-dlp failed: {detail}");
        }

        self.append_yt_dlp_output_line(format!(
            "yt-dlp finished with exit code: {} ({command_line})",
            status.code().unwrap_or(-1)
        ));
    }

    fn append_yt_dlp_stream_line(&mut self, stream: YtDlpStream, line: String) {
        let prefix = match stream {
            YtDlpStream::Stdout => "stdout",
            YtDlpStream::Stderr => "stderr",
        };
        self.yt_dlp_output.append_prefixed(prefix, line);
    }

    fn append_yt_dlp_output_line(&mut self, line: String) {
        self.yt_dlp_output.append_line(line);
    }
}

fn spawn_yt_dlp_reader<R>(reader: R, stream: YtDlpStream, tx: mpsc::Sender<YtDlpEvent>)
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
                        .send(YtDlpEvent::Chunk {
                            stream,
                            data: buf[..read].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(YtDlpEvent::ReaderError {
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
    running: &mut RunningYtDlp,
    stream: YtDlpStream,
    data: &[u8],
) -> Vec<String> {
    let (raw, pending) = match stream {
        YtDlpStream::Stdout => (&mut running.stdout_raw, &mut running.stdout_pending),
        YtDlpStream::Stderr => (&mut running.stderr_raw, &mut running.stderr_pending),
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
