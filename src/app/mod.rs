mod ffmpeg;
mod files;
mod input;
mod trim;

use std::{env, fs, io, path::PathBuf, process::Child, sync::mpsc::Receiver};

use crate::{
    media::{OUTPUT_FORMATS, VideoStats},
    model::{FileEntry, Focus, InputField, RightTab, TimeInput, VideoBounds},
};

use self::files::read_entries;

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
    pub(crate) remove_audio: bool,
    pub(crate) output_name: String,
    pub(crate) active_input: InputField,
    pub(crate) start_part: usize,
    pub(crate) end_part: usize,
    pub(crate) output_fps_cursor: usize,
    pub(crate) output_bitrate_cursor: usize,
    pub(crate) output_cursor: usize,
    pub(crate) selected_video_stats: Option<VideoStats>,
    selected_video_bounds: Option<VideoBounds>,
    pub(crate) status_message: String,
    pub(crate) ffmpeg_output: Vec<String>,
    pub(crate) ffmpeg_scroll: usize,
    pub(crate) show_keybinds: bool,
    pub(crate) ffmpeg_spinner_frame: usize,
    pub(crate) right_tab: RightTab,
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
    pub fn new(start_dir: Option<PathBuf>) -> io::Result<Self> {
        let cwd = resolve_start_dir(start_dir)?;
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
            output_fps: "30".to_string(),
            output_bitrate_kbps: "8000".to_string(),
            remove_audio: false,
            output_name: String::new(),
            active_input: InputField::Start,
            start_part: 0,
            end_part: 0,
            output_fps_cursor: 0,
            output_bitrate_cursor: 0,
            output_cursor: 0,
            selected_video_stats: None,
            selected_video_bounds: None,
            status_message: "Select a video file in the left pane.".to_string(),
            ffmpeg_output: vec!["ffmpeg output will appear here after trimming.".to_string()],
            ffmpeg_scroll: 0,
            show_keybinds: false,
            ffmpeg_spinner_frame: 0,
            right_tab: RightTab::Trim,
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

    pub fn right_tab(&self) -> RightTab {
        self.right_tab
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

    pub fn should_treat_digit_as_trim_input(&self, focus: Focus) -> bool {
        if focus != Focus::RightTop || self.right_tab != RightTab::Trim {
            return false;
        }

        matches!(
            self.active_input,
            InputField::Start
                | InputField::End
                | InputField::Fps
                | InputField::Bitrate
                | InputField::Output
        )
    }

    pub fn can_focus_right_bottom(&self) -> bool {
        self.right_tab == RightTab::Trim
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
