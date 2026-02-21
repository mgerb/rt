// Trim execution workflow.
// - Validates time range, format-specific options, and required output fields.
// - Translates current form state into ffmpeg CLI arguments.
// - Starts ffmpeg jobs and reports launch/validation errors back to the UI.
use crate::{
    media::{
        enforce_output_extension, next_available_output_path, resolve_output_path, shell_quote,
    },
    model::TimeInput,
};

use super::App;

impl App {
    pub fn trim_selected_video(&mut self) {
        if self.running_trim.is_some() {
            self.status_message = "ffmpeg is already running. Wait for it to finish.".to_string();
            return;
        }
        if !self.ffmpeg_available() {
            self.status_message =
                "ffmpeg was not found in PATH. Install ffmpeg to enable trimming.".to_string();
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

        let output_fps = self.output_fps.trim().to_string();
        let Some(parsed_output_fps) = parse_output_fps(&output_fps) else {
            self.status_message = "FPS must be a number greater than 0.".to_string();
            return;
        };
        let output_bitrate = self.output_bitrate_kbps.trim().to_string();
        let Some(parsed_output_bitrate_kbps) = parse_output_bitrate_kbps(&output_bitrate) else {
            self.status_message = "Bitrate must be a whole number greater than 0.".to_string();
            return;
        };

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
                format!("fps={parsed_output_fps}"),
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
                "-b:v".to_string(),
                format!("{parsed_output_bitrate_kbps}k"),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-r".to_string(),
                parsed_output_fps.clone(),
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
}

pub(super) fn default_output_fps(stats: Option<&crate::media::VideoStats>) -> String {
    if let Some(fps) = stats
        .map(|stats| stats.fps.trim())
        .filter(|fps| parse_output_fps(fps).is_some())
    {
        fps.to_string()
    } else {
        "30".to_string()
    }
}

pub(super) fn parse_output_fps(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = trimmed.parse::<f64>().ok()?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return None;
    }

    Some(trimmed.to_string())
}

fn parse_output_bitrate_kbps(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    trimmed.parse::<u32>().ok().filter(|bitrate| *bitrate > 0)
}
