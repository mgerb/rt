// Shared data model used across state, input handling, and rendering.
// - Defines app enums (focus targets, tabs, and active input fields).
// - Defines core value types like file entries and structured time input.
// - Keeps common types decoupled from module-specific logic.
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Left,
    RightTop,
    RightBottom,
}

impl Focus {
    pub fn next_window(self) -> Self {
        match self {
            Self::Left => Self::RightTop,
            Self::RightTop => Self::RightBottom,
            Self::RightBottom => Self::Left,
        }
    }

    pub fn previous_window(self) -> Self {
        match self {
            Self::Left => Self::RightBottom,
            Self::RightTop => Self::Left,
            Self::RightBottom => Self::RightTop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Trim,
    Hello,
}

impl RightTab {
    pub const ALL: [Self; 2] = [Self::Trim, Self::Hello];

    pub fn next(self) -> Self {
        match self {
            Self::Trim => Self::Hello,
            Self::Hello => Self::Trim,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Trim => Self::Hello,
            Self::Hello => Self::Trim,
        }
    }

    pub fn number(self) -> usize {
        match self {
            Self::Trim => 1,
            Self::Hello => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Trim => "Trim",
            Self::Hello => "Hello",
        }
    }

    pub fn from_number(number: usize) -> Option<Self> {
        match number {
            1 => Some(Self::Trim),
            2 => Some(Self::Hello),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    Start,
    End,
    Format,
    Fps,
    Bitrate,
    RemoveAudio,
    Output,
}

#[derive(Debug, Clone)]
pub struct TimeInput {
    hours: String,
    minutes: String,
    seconds: String,
}

impl TimeInput {
    pub fn zero() -> Self {
        Self {
            hours: "00".to_string(),
            minutes: "00".to_string(),
            seconds: "00".to_string(),
        }
    }

    pub fn from_seconds(seconds: f64) -> Self {
        let total = seconds.max(0.0).round() as u64;
        let hours = (total / 3600).min(99);
        let minutes = (total % 3600) / 60;
        let secs = total % 60;

        Self {
            hours: format!("{hours:02}"),
            minutes: format!("{minutes:02}"),
            seconds: format!("{secs:02}"),
        }
    }

    pub fn to_ffmpeg_timestamp(&self) -> String {
        format!("{}:{}:{}", self.hours, self.minutes, self.seconds)
    }

    pub fn to_seconds(&self) -> u32 {
        let hours = self.hours.parse::<u32>().unwrap_or(0);
        let minutes = self.minutes.parse::<u32>().unwrap_or(0);
        let seconds = self.seconds.parse::<u32>().unwrap_or(0);
        hours * 3600 + minutes * 60 + seconds
    }

    pub fn has_valid_minute_second_range(&self) -> bool {
        let minutes = self.minutes.parse::<u32>().unwrap_or(99);
        let seconds = self.seconds.parse::<u32>().unwrap_or(99);
        minutes < 60 && seconds < 60
    }

    pub fn part(&self, part_index: usize) -> &str {
        match part_index {
            0 => &self.hours,
            1 => &self.minutes,
            2 => &self.seconds,
            _ => "00",
        }
    }

    pub fn push_digit_to_part(&mut self, part_index: usize, digit: char) {
        if !digit.is_ascii_digit() {
            return;
        }

        self.ensure_two_digit_parts();

        if let Some(part) = self.part_mut(part_index) {
            let ones = part.chars().nth(1).unwrap_or('0');
            *part = format!("{ones}{digit}");
        }
    }

    pub fn clear_part(&mut self, part_index: usize) {
        if let Some(part) = self.part_mut(part_index) {
            *part = "00".to_string();
        }
    }

    fn ensure_two_digit_parts(&mut self) {
        if self.hours.len() != 2 || !self.hours.chars().all(|ch| ch.is_ascii_digit()) {
            self.hours = "00".to_string();
        }
        if self.minutes.len() != 2 || !self.minutes.chars().all(|ch| ch.is_ascii_digit()) {
            self.minutes = "00".to_string();
        }
        if self.seconds.len() != 2 || !self.seconds.chars().all(|ch| ch.is_ascii_digit()) {
            self.seconds = "00".to_string();
        }
    }

    fn part_mut(&mut self, part_index: usize) -> Option<&mut String> {
        match part_index {
            0 => Some(&mut self.hours),
            1 => Some(&mut self.minutes),
            2 => Some(&mut self.seconds),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoBounds {
    pub start_seconds: u32,
    pub end_seconds: u32,
}
