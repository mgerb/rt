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
pub enum InputField {
    Start,
    End,
    Output,
}

impl InputField {
    pub fn next(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Output,
            Self::Output => Self::Start,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Start => Self::Output,
            Self::End => Self::Start,
            Self::Output => Self::End,
        }
    }
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

    pub fn set_digit_at(&mut self, index: usize, digit: char) {
        if !digit.is_ascii_digit() {
            return;
        }

        self.ensure_two_digit_parts();

        match index {
            0 => self.hours.replace_range(0..1, &digit.to_string()),
            1 => self.hours.replace_range(1..2, &digit.to_string()),
            2 => self.minutes.replace_range(0..1, &digit.to_string()),
            3 => self.minutes.replace_range(1..2, &digit.to_string()),
            4 => self.seconds.replace_range(0..1, &digit.to_string()),
            5 => self.seconds.replace_range(1..2, &digit.to_string()),
            _ => {}
        }
    }

    pub fn digit_at(&self, index: usize) -> char {
        let (part, offset) = match index {
            0 => (&self.hours, 0),
            1 => (&self.hours, 1),
            2 => (&self.minutes, 0),
            3 => (&self.minutes, 1),
            4 => (&self.seconds, 0),
            5 => (&self.seconds, 1),
            _ => return '0',
        };

        part.as_bytes()
            .get(offset)
            .map(|byte| *byte as char)
            .unwrap_or('0')
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
}

#[derive(Debug, Clone, Copy)]
pub struct VideoBounds {
    pub start_seconds: u32,
    pub end_seconds: u32,
}
