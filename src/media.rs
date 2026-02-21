// Media and path helper functions.
// - Uses ffprobe to gather timing bounds and display stats for selected videos.
// - Parses/normalizes probed values (fps, bitrate, duration, size).
// - Handles output filename/extension rules and numbered collision resolution.
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{TimeInput, VideoBounds};

pub const OUTPUT_FORMATS: [&str; 8] = ["mp4", "mov", "mkv", "gif", "mp3", "m4a", "wav", "flac"];

pub fn is_audio_output_format(format: &str) -> bool {
    matches!(
        normalize_output_format(format),
        "mp3" | "m4a" | "wav" | "flac"
    )
}

#[derive(Debug, Clone)]
pub struct VideoStats {
    pub duration: String,
    pub resolution: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub size: String,
    pub bitrate: String,
    pub bitrate_kbps: Option<u32>,
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

pub fn is_audio_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "m4a" | "wav" | "flac" | "aac" | "ogg" | "opus" | "wma"
    )
}

pub fn is_editable_media_file(path: &Path) -> bool {
    is_video_file(path) || is_audio_file(path)
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
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

pub fn output_format_for_path(path: &Path) -> &'static str {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(normalize_output_format)
        .unwrap_or(OUTPUT_FORMATS[0])
}

pub fn enforce_output_extension(output_name: &str, output_format: &str) -> String {
    let format = normalize_output_format(output_format);

    if output_name.trim().is_empty() {
        return format!("output.{format}");
    }

    let mut path = PathBuf::from(output_name);
    if path.extension().is_some() {
        path.set_extension(format);
        path.to_string_lossy().into_owned()
    } else {
        format!("{output_name}.{format}")
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

pub fn next_available_output_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let (base_stem, parsed_number) = split_numbered_suffix(&stem);
    let mut number = parsed_number
        .and_then(|value| value.checked_add(1))
        .unwrap_or(1);

    loop {
        let candidate_name = if extension.is_empty() {
            format!("{base_stem}({number})")
        } else {
            format!("{base_stem}({number}).{extension}")
        };
        let candidate_path = parent.join(candidate_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
        number = number.saturating_add(1);
    }
}

pub fn output_path_without_numbered_suffix(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let (base_stem, _) = split_numbered_suffix(&stem);

    if extension.is_empty() {
        parent.join(base_stem)
    } else {
        parent.join(format!("{base_stem}.{extension}"))
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
    let width = stats_map
        .get("width")
        .and_then(|value| value.parse::<u32>().ok());
    let height = stats_map
        .get("height")
        .and_then(|value| value.parse::<u32>().ok());
    let resolution = match (width, height) {
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
    let bitrate_bits_per_second = stats_map
        .get("bit_rate")
        .and_then(|value| value.parse::<u64>().ok());
    let bitrate = bitrate_bits_per_second
        .map(format_bitrate)
        .unwrap_or_else(|| "n/a".to_string());
    let bitrate_kbps = bitrate_bits_per_second.and_then(bitrate_kbps_from_bits_per_second);

    let audio_codec = probe_audio_codec(path).unwrap_or_else(|_| "n/a".to_string());

    Ok(VideoStats {
        duration,
        resolution,
        width,
        height,
        fps,
        video_codec,
        audio_codec,
        size,
        bitrate,
        bitrate_kbps,
    })
}

pub fn scaled_resolution_for_percent(width: u32, height: u32, percent: u32) -> (u32, u32) {
    if percent == 100 {
        return (width, height);
    }

    (
        scaled_dimension_for_percent(width, percent),
        scaled_dimension_for_percent(height, percent),
    )
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

pub fn normalize_output_format(value: &str) -> &'static str {
    OUTPUT_FORMATS
        .iter()
        .copied()
        .find(|format| format.eq_ignore_ascii_case(value))
        .unwrap_or(OUTPUT_FORMATS[0])
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

fn split_numbered_suffix(stem: &str) -> (String, Option<u32>) {
    let Some(without_close) = stem.strip_suffix(')') else {
        return (stem.to_string(), None);
    };

    let Some(open_index) = without_close.rfind('(') else {
        return (stem.to_string(), None);
    };

    let number_text = &without_close[open_index + 1..];
    if number_text.is_empty() || !number_text.chars().all(|ch| ch.is_ascii_digit()) {
        return (stem.to_string(), None);
    }

    let Some(number) = number_text.parse::<u32>().ok() else {
        return (stem.to_string(), None);
    };

    let base = &without_close[..open_index];
    if base.is_empty() {
        return (stem.to_string(), None);
    }

    (base.to_string(), Some(number))
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

fn scaled_dimension_for_percent(dimension: u32, percent: u32) -> u32 {
    let mut scaled = ((dimension as u64 * percent as u64) + 50) / 100;
    if scaled < 2 {
        scaled = 2;
    }
    let mut scaled = scaled as u32;

    if scaled % 2 == 1 {
        scaled = scaled.saturating_sub(1);
    }

    scaled.max(2)
}

fn bitrate_kbps_from_bits_per_second(bits_per_second: u64) -> Option<u32> {
    if bits_per_second == 0 {
        return None;
    }

    let kbps = ((bits_per_second + 500) / 1_000).max(1);
    u32::try_from(kbps).ok()
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
