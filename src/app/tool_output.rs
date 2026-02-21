// Reusable tool-output state and behavior.
// - Stores output lines for any tool process panel.
// - Implements shared scrolling, paging, and tail-follow behavior.
// - Provides helpers for common command/output line formatting.
#[derive(Debug, Clone)]
pub(crate) struct ToolOutput {
    lines: Vec<String>,
    scroll: usize,
    follow_tail: bool,
}

impl ToolOutput {
    const PAGE_STEP: usize = 12;

    pub(crate) fn empty() -> Self {
        Self {
            lines: Vec::new(),
            scroll: 0,
            follow_tail: true,
        }
    }

    pub(crate) fn begin_stream(&mut self, command_line: &str, streaming_message: &str) {
        self.lines = vec![format!("$ {command_line}"), streaming_message.to_string()];
        self.scroll = self.lines.len().saturating_sub(1);
        self.follow_tail = true;
    }

    pub(crate) fn replace_with_command_error(&mut self, command_line: &str, error_message: &str) {
        self.lines = vec![format!("$ {command_line}"), error_message.to_string()];
        self.scroll = 0;
        self.follow_tail = true;
    }

    pub(crate) fn append_prefixed(&mut self, prefix: &str, line: String) {
        self.append_line(format!("{prefix}: {line}"));
    }

    pub(crate) fn append_line(&mut self, line: String) {
        self.lines.push(line);
        if self.follow_tail {
            self.scroll = self.lines.len().saturating_sub(1);
        }
    }

    pub(crate) fn scroll_down(&mut self) {
        let max_scroll = self.lines.len().saturating_sub(1);
        if self.scroll < max_scroll {
            self.scroll += 1;
        }
        self.follow_tail = self.scroll >= max_scroll;
    }

    pub(crate) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
        self.follow_tail = false;
    }

    pub(crate) fn page_down(&mut self) {
        let max_scroll = self.lines.len().saturating_sub(1);
        self.scroll = (self.scroll + Self::PAGE_STEP).min(max_scroll);
        self.follow_tail = self.scroll >= max_scroll;
    }

    pub(crate) fn page_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(Self::PAGE_STEP);
        self.follow_tail = false;
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }
}
