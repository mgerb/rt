use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{TimeInput, VideoBounds};

pub fn is_video_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "mpeg" | "mpg" | "wmv" | "flv"
    )
}

pub fn default_output_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let ext = path
        .extension()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    if ext.is_empty() {
        format!("{stem}_copy")
    } else {
        format!("{stem}_copy.{ext}")
    }
}

pub fn resolve_output_path(input_path: &Path, output_name: &str) -> PathBuf {
    let candidate = PathBuf::from(output_name);
    let has_separator = output_name.contains('/') || output_name.contains('\\');

    if candidate.is_absolute() || has_separator {
        candidate
    } else {
        input_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    }
}

pub fn probe_video_times(path: &Path) -> io::Result<(TimeInput, TimeInput, VideoBounds)> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=start_time,duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other("ffprobe failed"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());

    let start_secs = lines
        .next()
        .and_then(parse_probe_seconds)
        .unwrap_or(0.0)
        .max(0.0);
    let duration_secs = lines.next().and_then(parse_probe_seconds).unwrap_or(0.0);
    let end_secs = (start_secs + duration_secs).max(start_secs);

    let bounds = VideoBounds {
        start_seconds: start_secs.round() as u32,
        end_seconds: end_secs.round() as u32,
    };

    Ok((
        TimeInput::from_seconds(start_secs),
        TimeInput::from_seconds(end_secs),
        bounds,
    ))
}

pub fn summarize_ffmpeg_error(stderr: &str) -> String {
    let lines = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    for line in lines.iter().rev() {
        if *line != "Conversion failed!" {
            return (*line).to_string();
        }
    }

    "unknown ffmpeg error".to_string()
}

pub fn collect_ffmpeg_lines(command_line: &str, stdout: &[u8], stderr: &[u8]) -> Vec<String> {
    let mut lines = vec![format!("$ {command_line}"), String::new()];

    lines.push("stderr:".to_string());
    for line in String::from_utf8_lossy(stderr).lines() {
        lines.push(line.to_string());
    }

    lines.push(String::new());
    lines.push("stdout:".to_string());
    for line in String::from_utf8_lossy(stdout).lines() {
        lines.push(line.to_string());
    }

    if lines.iter().all(|line| line.trim().is_empty()) {
        vec!["(no ffmpeg output)".to_string()]
    } else {
        lines
    }
}

pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:+@=?".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn parse_probe_seconds(value: &str) -> Option<f64> {
    if value.eq_ignore_ascii_case("n/a") {
        return None;
    }

    let parsed = value.parse::<f64>().ok()?;
    if parsed.is_finite() {
        Some(parsed)
    } else {
        None
    }
}
