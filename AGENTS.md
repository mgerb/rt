# AGENTS.md

## Project Summary

- Rust TUI app (`ratatui`) that wraps various tools.
- Left pane: file browser. Right pane: various tools + tool output.

## Run / Build

- Run: `cargo run`
- Check: `cargo check`

## Practical Notes

- `ffmpeg` and `ffprobe` must be installed.
- Large local video files may exist untracked; `jj` may warn about snapshot size limits for them.

## Development notes

- Make sure to document the code with comments so that a human can understand it.
- When creating new components consider making them reusable so that they can be
  reused across existing (or new) tools.
- When making commits use the conventional commits strategy for commit messages.
- Use `jj` to make commits.
