use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{TimeInput, VideoBounds};

#[derive(Debug, Clone)]
pub struct VideoStats {
    pub duration: String,
    pub resolution: String,
    pub fps: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub size: String,
    pub bitrate: String,
}

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

pub fn probe_video_stats(path: &Path) -> io::Result<VideoStats> {
    let video_output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=codec_name,width,height,avg_frame_rate")
        .arg("-show_entries")
        .arg("format=duration,size,bit_rate")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=0")
        .arg(path)
        .output()?;

    if !video_output.status.success() {
        return Err(io::Error::other("ffprobe stats failed"));
    }

    let stats_map = parse_key_value_lines(&String::from_utf8_lossy(&video_output.stdout));

    let video_codec = stats_map
        .get("codec_name")
        .cloned()
        .unwrap_or_else(|| "n/a".to_string());
    let resolution = match (stats_map.get("width"), stats_map.get("height")) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "n/a".to_string(),
    };
    let fps = stats_map
        .get("avg_frame_rate")
        .and_then(|value| parse_fraction(value))
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "n/a".to_string());
    let duration = stats_map
        .get("duration")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| TimeInput::from_seconds(seconds).to_ffmpeg_timestamp())
        .unwrap_or_else(|| "n/a".to_string());
    let size = stats_map
        .get("size")
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_bytes)
        .unwrap_or_else(|| "n/a".to_string());
    let bitrate = stats_map
        .get("bit_rate")
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_bitrate)
        .unwrap_or_else(|| "n/a".to_string());

    let audio_codec = probe_audio_codec(path).unwrap_or_else(|_| "n/a".to_string());

    Ok(VideoStats {
        duration,
        resolution,
        fps,
        video_codec,
        audio_codec,
        size,
        bitrate,
    })
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

fn parse_key_value_lines(input: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn parse_fraction(value: &str) -> Option<f64> {
    if value.eq_ignore_ascii_case("n/a") {
        return None;
    }
    let (num, den) = value.split_once('/')?;
    let num = num.parse::<f64>().ok()?;
    let den = den.parse::<f64>().ok()?;
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;

    if bytes_f >= GB {
        format!("{:.2} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.2} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.2} KB", bytes_f / KB)
    } else {
        format!("{bytes} B")
    }
}

fn format_bitrate(bits_per_second: u64) -> String {
    let bps = bits_per_second as f64;
    if bps >= 1_000_000.0 {
        format!("{:.2} Mbps", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.2} Kbps", bps / 1_000.0)
    } else {
        format!("{bits_per_second} bps")
    }
}

fn probe_audio_codec(path: &Path) -> io::Result<String> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=codec_name")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other("ffprobe audio codec failed"));
    }

    let codec = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("n/a")
        .to_string();
    Ok(codec)
}
