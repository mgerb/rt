// Reusable tool-output state and behavior.
// - Stores output lines for any tool process panel.
// - Implements shared scrolling, paging, and tail-follow behavior.
// - Provides helpers for common command/output line formatting.
use std::cell::Cell;

#[derive(Debug, Clone)]
pub(crate) struct ToolOutput {
    lines: Vec<String>,
    scroll: Cell<usize>,
    last_max_scroll_top: Cell<usize>,
    follow_tail: bool,
}

impl ToolOutput {
    const PAGE_STEP: usize = 12;
    const MAX_LINES: usize = 20_000;

    pub(crate) fn empty() -> Self {
        Self {
            lines: Vec::new(),
            scroll: Cell::new(0),
            last_max_scroll_top: Cell::new(0),
            follow_tail: true,
        }
    }

    pub(crate) fn begin_stream(&mut self, command_line: &str, streaming_message: &str) {
        self.lines = vec![format!("$ {command_line}"), streaming_message.to_string()];
        self.scroll.set(self.lines.len().saturating_sub(1));
        self.follow_tail = true;
    }

    pub(crate) fn replace_with_command_error(&mut self, command_line: &str, error_message: &str) {
        self.lines = vec![format!("$ {command_line}"), error_message.to_string()];
        self.scroll.set(0);
        self.follow_tail = true;
    }

    pub(crate) fn append_prefixed(&mut self, prefix: &str, line: String) {
        self.append_line(format!("{prefix}: {line}"));
    }

    pub(crate) fn append_line(&mut self, line: String) {
        self.lines.push(line);
        self.trim_old_lines_if_needed();
        if self.follow_tail {
            self.scroll.set(self.lines.len().saturating_sub(1));
        }
    }

    pub(crate) fn scroll_down(&mut self) {
        let max_scroll = self.last_max_scroll_top.get();
        let next = (self.scroll.get() + 1).min(max_scroll);
        self.scroll.set(next);
        if next >= max_scroll {
            self.follow_tail = true;
        }
    }

    pub(crate) fn scroll_up(&mut self) {
        self.scroll.set(self.scroll.get().saturating_sub(1));
        self.follow_tail = false;
    }

    pub(crate) fn page_down(&mut self) {
        let max_scroll = self.last_max_scroll_top.get();
        let next = (self.scroll.get() + Self::PAGE_STEP).min(max_scroll);
        self.scroll.set(next);
        if next >= max_scroll {
            self.follow_tail = true;
        }
    }

    pub(crate) fn page_up(&mut self) {
        self.scroll
            .set(self.scroll.get().saturating_sub(Self::PAGE_STEP));
        self.follow_tail = false;
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) fn scroll(&self) -> usize {
        self.scroll.get()
    }

    pub(crate) fn clamped_scroll_for_viewport(&self, visible_line_count: usize) -> usize {
        let visible_line_count = visible_line_count.max(1);
        let max_scroll_top = self.lines.len().saturating_sub(visible_line_count);
        self.last_max_scroll_top.set(max_scroll_top);
        let clamped = self.scroll().min(max_scroll_top);
        self.scroll.set(clamped);
        clamped
    }

    fn trim_old_lines_if_needed(&mut self) {
        if self.lines.len() <= Self::MAX_LINES {
            return;
        }

        let overflow = self.lines.len() - Self::MAX_LINES;
        self.lines.drain(0..overflow);
        self.scroll.set(self.scroll.get().saturating_sub(overflow));
    }
}
