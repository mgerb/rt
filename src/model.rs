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
pub enum TimeSection {
    Hours,
    Minutes,
    Seconds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    Start(TimeSection),
    End(TimeSection),
    Output,
}

impl InputField {
    pub fn next(self) -> Self {
        match self {
            Self::Start(TimeSection::Hours) => Self::Start(TimeSection::Minutes),
            Self::Start(TimeSection::Minutes) => Self::Start(TimeSection::Seconds),
            Self::Start(TimeSection::Seconds) => Self::End(TimeSection::Hours),
            Self::End(TimeSection::Hours) => Self::End(TimeSection::Minutes),
            Self::End(TimeSection::Minutes) => Self::End(TimeSection::Seconds),
            Self::End(TimeSection::Seconds) => Self::Output,
            Self::Output => Self::Start(TimeSection::Hours),
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Start(TimeSection::Hours) => Self::Output,
            Self::Start(TimeSection::Minutes) => Self::Start(TimeSection::Hours),
            Self::Start(TimeSection::Seconds) => Self::Start(TimeSection::Minutes),
            Self::End(TimeSection::Hours) => Self::Start(TimeSection::Seconds),
            Self::End(TimeSection::Minutes) => Self::End(TimeSection::Hours),
            Self::End(TimeSection::Seconds) => Self::End(TimeSection::Minutes),
            Self::Output => Self::End(TimeSection::Seconds),
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

    pub fn part(&self, section: TimeSection) -> &str {
        match section {
            TimeSection::Hours => &self.hours,
            TimeSection::Minutes => &self.minutes,
            TimeSection::Seconds => &self.seconds,
        }
    }

    pub fn push_digit(&mut self, section: TimeSection, digit: char) {
        let part = self.part_mut(section);
        if part.len() != 2 || !part.chars().all(|c| c.is_ascii_digit()) {
            *part = "00".to_string();
        }

        let second_char = part.as_bytes()[1] as char;
        *part = format!("{second_char}{digit}");
    }

    pub fn backspace(&mut self, section: TimeSection) {
        let part = self.part_mut(section);
        if part.len() != 2 || !part.chars().all(|c| c.is_ascii_digit()) {
            *part = "00".to_string();
        }

        let first_char = part.as_bytes()[0] as char;
        *part = format!("0{first_char}");
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

    fn part_mut(&mut self, section: TimeSection) -> &mut String {
        match section {
            TimeSection::Hours => &mut self.hours,
            TimeSection::Minutes => &mut self.minutes,
            TimeSection::Seconds => &mut self.seconds,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoBounds {
    pub start_seconds: u32,
    pub end_seconds: u32,
}
