// Central application state shared by the app submodules.
// - Stores file-browser state, editor form inputs, tab/focus state, and output logs.
// - Owns background ffmpeg job state and process communication handles.
// - Exposes cross-cutting helpers used by event handling and rendering code.
mod ffmpeg;
mod files;
mod input;
mod tool_output;
mod editor;
mod downloader;

use std::{
    env, fs, io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::Receiver,
};

use crate::{
    media::{OUTPUT_FORMATS, VideoStats, is_audio_output_format},
    model::{FileEntry, Focus, InputField, RightTab, TimeInput, VideoBounds},
};

use self::files::read_entries;
use self::tool_output::ToolOutput;

pub struct App {
    pub(crate) cwd: PathBuf,
    initial_dir: PathBuf,
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) selected: usize,
    pub(crate) selected_video: Option<PathBuf>,
    pub(crate) start_time: TimeInput,
    pub(crate) end_time: TimeInput,
    pub(crate) output_format: &'static str,
    pub(crate) output_fps: String,
    pub(crate) output_bitrate_kbps: String,
    pub(crate) output_scale_percent: String,
    use_gpu_encoding: bool,
    pub(crate) remove_audio: bool,
    pub(crate) output_name: String,
    pub(crate) active_input: InputField,
    pub(crate) start_part: usize,
    pub(crate) end_part: usize,
    pub(crate) output_fps_cursor: usize,
    pub(crate) output_bitrate_cursor: usize,
    pub(crate) output_scale_percent_cursor: usize,
    pub(crate) output_cursor: usize,
    pub(crate) overwrite_fps_on_next_type: bool,
    pub(crate) overwrite_bitrate_on_next_type: bool,
    pub(crate) overwrite_scale_percent_on_next_type: bool,
    pub(crate) selected_video_stats: Option<VideoStats>,
    selected_video_bounds: Option<VideoBounds>,
    pub(crate) status_message: String,
    pub(crate) ffmpeg_output: ToolOutput,
    pub(crate) downloader_url: String,
    pub(crate) downloader_url_cursor: usize,
    pub(crate) downloader_output: ToolOutput,
    ffmpeg_available: bool,
    downloader_available: bool,
    gpu_h264_encoder_available: bool,
    pub(crate) show_keybinds: bool,
    pub(crate) ffmpeg_spinner_frame: usize,
    pub(crate) downloader_spinner_frame: usize,
    pub(crate) right_tab: RightTab,
    pending_delete: Option<PendingDelete>,
    pending_cancel: Option<PendingCancel>,
    running_editor: Option<RunningEditor>,
    running_downloader: Option<RunningDownloader>,
}

struct PendingDelete {
    name: String,
    path: PathBuf,
}

enum PendingCancel {
    Editor,
    Downloader,
}

struct RunningEditor {
    child: Child,
    rx: Receiver<FfmpegEvent>,
    command_line: String,
    output_path: PathBuf,
    stdout_raw: Vec<u8>,
    stderr_raw: Vec<u8>,
    stdout_pending: Vec<u8>,
    stderr_pending: Vec<u8>,
}

struct RunningDownloader {
    child: Child,
    rx: Receiver<DownloaderEvent>,
    command_line: String,
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

#[derive(Clone, Copy)]
enum DownloaderStream {
    Stdout,
    Stderr,
}

enum DownloaderEvent {
    Chunk { stream: DownloaderStream, data: Vec<u8> },
    ReaderError { stream: DownloaderStream, error: String },
}

impl App {
    pub fn new(start_dir: Option<PathBuf>) -> io::Result<Self> {
        let cwd = resolve_start_dir(start_dir)?;
        let entries = read_entries(&cwd)?;
        let ffmpeg_available = detect_ffmpeg_available();
        let downloader_available = detect_downloader_available();
        let gpu_h264_encoder_available = if ffmpeg_available {
            detect_ffmpeg_encoder_available("h264_nvenc")
        } else {
            false
        };

        Ok(Self {
            cwd: cwd.clone(),
            initial_dir: cwd,
            entries,
            selected: 0,
            selected_video: None,
            start_time: TimeInput::zero(),
            end_time: TimeInput::zero(),
            output_format: OUTPUT_FORMATS[0],
            output_fps: "30".to_string(),
            output_bitrate_kbps: "8000".to_string(),
            output_scale_percent: "100".to_string(),
            use_gpu_encoding: gpu_h264_encoder_available,
            remove_audio: false,
            output_name: String::new(),
            active_input: InputField::Start,
            start_part: 0,
            end_part: 0,
            output_fps_cursor: 0,
            output_bitrate_cursor: 0,
            output_scale_percent_cursor: 3,
            output_cursor: 0,
            overwrite_fps_on_next_type: true,
            overwrite_bitrate_on_next_type: true,
            overwrite_scale_percent_on_next_type: true,
            selected_video_stats: None,
            selected_video_bounds: None,
            status_message: "Select a video file in the left pane.".to_string(),
            ffmpeg_output: ToolOutput::empty(),
            downloader_url: String::new(),
            downloader_url_cursor: 0,
            downloader_output: ToolOutput::empty(),
            ffmpeg_available,
            downloader_available,
            gpu_h264_encoder_available,
            show_keybinds: false,
            ffmpeg_spinner_frame: 0,
            downloader_spinner_frame: 0,
            right_tab: RightTab::Editor,
            pending_delete: None,
            pending_cancel: None,
            running_editor: None,
            running_downloader: None,
        })
    }

    pub fn toggle_keybinds(&mut self) {
        self.show_keybinds = !self.show_keybinds;
    }

    pub fn hide_keybinds(&mut self) {
        self.show_keybinds = false;
    }

    pub fn tick(&mut self) {
        if self.running_editor.is_some() {
            self.ffmpeg_spinner_frame = (self.ffmpeg_spinner_frame + 1) % spinner_frames().len();
            self.pump_running_editor_events();
            self.try_finish_running_editor();
        }

        if self.running_downloader.is_some() {
            self.downloader_spinner_frame = (self.downloader_spinner_frame + 1) % spinner_frames().len();
            self.pump_running_downloader_events();
            self.try_finish_running_downloader();
        }
    }

    pub fn ffmpeg_available(&self) -> bool {
        self.ffmpeg_available
    }

    pub fn ffmpeg_output_lines(&self) -> &[String] {
        self.ffmpeg_output.lines()
    }

    pub fn ffmpeg_output_scroll(&self) -> usize {
        self.ffmpeg_output.scroll()
    }

    pub fn downloader_available(&self) -> bool {
        self.downloader_available
    }

    pub fn downloader_output_lines(&self) -> &[String] {
        self.downloader_output.lines()
    }

    pub fn downloader_output_scroll(&self) -> usize {
        self.downloader_output.scroll()
    }

    pub fn gpu_h264_encoder_available(&self) -> bool {
        self.gpu_h264_encoder_available
    }

    pub fn right_tab(&self) -> RightTab {
        self.right_tab
    }

    pub fn is_gif_output(&self) -> bool {
        self.output_format == "gif"
    }

    pub fn audio_only_output_selected(&self) -> bool {
        is_audio_output_format(self.output_format)
    }

    pub fn bitrate_enabled(&self) -> bool {
        !self.is_gif_output() && !self.audio_only_output_selected()
    }

    pub fn video_options_enabled(&self) -> bool {
        !self.audio_only_output_selected()
    }

    pub fn select_next_right_tab(&mut self) {
        self.right_tab = self.right_tab.next();
    }

    pub fn select_previous_right_tab(&mut self) {
        self.right_tab = self.right_tab.previous();
    }

    pub fn select_right_tab_by_number(&mut self, number: usize) -> bool {
        let Some(tab) = RightTab::from_number(number) else {
            return false;
        };
        self.right_tab = tab;
        true
    }

    pub fn should_treat_digit_as_editor_input(&self, focus: Focus) -> bool {
        if focus != Focus::RightTop {
            return false;
        }

        if self.right_tab == RightTab::Downloader {
            return true;
        }
        if self.right_tab != RightTab::Editor {
            return false;
        }

        matches!(
            self.active_input,
            InputField::Start
                | InputField::End
                | InputField::Fps
                | InputField::Bitrate
                | InputField::ScalePercent
                | InputField::Output
        )
    }

    pub fn can_focus_right_bottom(&self) -> bool {
        matches!(self.right_tab, RightTab::Editor | RightTab::Downloader)
    }

    pub fn normalize_focus(&self, focus: &mut Focus) {
        if !self.can_focus_right_bottom() && *focus == Focus::RightBottom {
            *focus = Focus::RightTop;
        }
    }

    pub fn next_focus(&self, current: Focus) -> Focus {
        if self.can_focus_right_bottom() {
            current.next_window()
        } else {
            match current {
                Focus::Left => Focus::RightTop,
                Focus::RightTop | Focus::RightBottom => Focus::Left,
            }
        }
    }

    pub fn previous_focus(&self, current: Focus) -> Focus {
        if self.can_focus_right_bottom() {
            current.previous_window()
        } else {
            match current {
                Focus::Left => Focus::RightTop,
                Focus::RightTop | Focus::RightBottom => Focus::Left,
            }
        }
    }

    pub fn has_pending_delete(&self) -> bool {
        self.pending_delete.is_some()
    }

    pub fn pending_delete_target(&self) -> Option<(&str, &std::path::Path)> {
        self.pending_delete
            .as_ref()
            .map(|pending| (pending.name.as_str(), pending.path.as_path()))
    }

    pub fn has_pending_cancel(&self) -> bool {
        self.pending_cancel.is_some()
    }

    pub fn pending_cancel_label(&self) -> Option<&'static str> {
        match self.pending_cancel {
            Some(PendingCancel::Editor) => Some("Editor export"),
            Some(PendingCancel::Downloader) => Some("Downloader job"),
            None => None,
        }
    }

    pub fn request_cancel_for_focused_tool(&mut self) {
        self.pending_cancel = match self.right_tab {
            RightTab::Editor if self.running_editor.is_some() => Some(PendingCancel::Editor),
            RightTab::Downloader if self.running_downloader.is_some() => {
                Some(PendingCancel::Downloader)
            }
            RightTab::Editor => {
                self.status_message = "No running editor export to cancel.".to_string();
                None
            }
            RightTab::Downloader => {
                self.status_message = "No running downloader job to cancel.".to_string();
                None
            }
        };
    }

    pub fn cancel_pending_cancel(&mut self) {
        self.pending_cancel = None;
    }

    pub fn confirm_pending_cancel(&mut self) {
        let Some(target) = self.pending_cancel.take() else {
            return;
        };

        match target {
            PendingCancel::Editor => self.cancel_editor_export(),
            PendingCancel::Downloader => self.cancel_downloader(),
        }
    }
}

fn resolve_start_dir(start_dir: Option<PathBuf>) -> io::Result<PathBuf> {
    let Some(path) = start_dir else {
        return env::current_dir();
    };

    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };

    let metadata = fs::metadata(&absolute).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid start directory '{}': {err}", absolute.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Start path is not a directory: {}", absolute.display()),
        ));
    }

    Ok(absolute)
}

fn spinner_frames() -> &'static [char] {
    &['|', '/', '-', '\\']
}

// Check once at startup so the UI can show a clear warning without spawning
// a process on every draw.
fn detect_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn detect_downloader_available() -> bool {
    Command::new("yt-dlp")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn detect_ffmpeg_encoder_available(encoder_name: &str) -> bool {
    let Ok(output) = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .any(|line| line.split_whitespace().any(|word| word == encoder_name))
}
