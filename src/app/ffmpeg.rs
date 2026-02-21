// ffmpeg process runtime management.
// - Spawns ffmpeg with piped stdout/stderr and streams output incrementally.
// - Updates in-memory output lines used by the log panel in real time.
// - Finalizes run status, refreshes file list after successful outputs,
//   and appends a full per-run transcript to ffmpeg_runs.log.
use std::{
    fs::OpenOptions,
    io::{self, BufReader, Read, Write},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::media::summarize_ffmpeg_error;

use super::{App, FfmpegEvent, FfmpegStream, RunningTrim};

impl App {
    pub(super) fn start_ffmpeg_job(
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
        self.ffmpeg_output
            .begin_stream(&command_line, "Streaming ffmpeg output...");
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

    pub(super) fn pump_running_trim_events(&mut self) {
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

    pub(super) fn try_finish_running_trim(&mut self) {
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
        self.ffmpeg_output.append_prefixed(prefix, line);
    }

    fn append_ffmpeg_output_line(&mut self, line: String) {
        self.ffmpeg_output.append_line(line);
    }

    pub(super) fn append_ffmpeg_run_log(
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
