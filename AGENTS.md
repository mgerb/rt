# AGENTS.md

## Project Summary
- Rust TUI app (`ratatui`) that wraps `ffmpeg` to trim video clips.
- Left pane: file browser. Right pane: trim form + ffmpeg output log.
- Main workflow: select video -> adjust start/end/output -> run ffmpeg.

## Run / Build
- Run: `cargo run`
- Check: `cargo check`

## Code Layout
- `src/main.rs`: event loop + key handling only.
- `src/app.rs`: app state and business logic (navigation, editing, trim execution, logging).
- `src/ui.rs`: rendering for panes + keybind popup.
- `src/media.rs`: ffprobe/ffmpeg helpers, parsing, formatting.
- `src/model.rs`: shared data types (`Focus`, `InputField`, `TimeInput`, etc.).

## Current UX / Behavior
- File browser shows `[D]/[V]/[F]`; video rows are colored.
- `?` toggles keybind popup.
- Footer hint: "Press ? to see keyboard shortcuts".
- Right side layout is dynamic:
  - ffmpeg output pane is small by default.
  - ffmpeg output pane expands when it has focus.
- Focus is visually clear via colored borders:
  - Files = blue, Trim = yellow, ffmpeg output = magenta.

## Input Model (Trim Pane)
- Fields: `Start`, `End`, `Output`.
- `Tab` / `Shift+Tab` moves between fields.
- `h/l` or `Left/Right` moves cursor inside active field.
- Time fields are `HH:MM:SS` with digit-level cursor editing.
- Output filename supports character insert/delete at cursor.

## ffmpeg / Logging
- Trim executes ffmpeg with re-encode settings (`libx264` + `aac`) for compatibility.
- Command + stdout/stderr are shown in ffmpeg output pane.
- Every run appends to `ffmpeg_runs.log` in repo root.

## Data / Validation
- On video select, app probes:
  - trim bounds (`start`, `end`) and
  - video stats (duration, resolution, fps, codecs, size, bitrate).
- Start/end are validated against probed bounds before running ffmpeg.

## Practical Notes
- `ffmpeg` and `ffprobe` must be installed.
- Large local video files may exist untracked; `jj` may warn about snapshot size limits for them.
