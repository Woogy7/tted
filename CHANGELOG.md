# Changelog

All notable changes are documented here.

## Unreleased

- Add inline and fenced backtick completion, including selected-text wrapping,
  language-labelled fences, and typing past closing delimiters.
- Ship a `te` launcher alongside `tted` in Cargo installs and release archives;
  the installer also provides `te` for older archives.
- Reuse cached Markdown styles in source view to avoid cold regex-highlighting
  stalls. Skip first-line syntax detection for empty buffers and log slow syntax
  passes without recording document contents.

- Continue Markdown bullets, numbered lists, tasks, and quotes on Enter; exit
  empty items, add continuation lines with Shift+Enter, and indent selected list
  lines with Tab/Shift+Tab as one undoable edit.
- Add remappable line-edge selection, Alt+Shift+Left/Right shortcuts, and an
  explicit key inspector for terminal/multiplexer troubleshooting.

- Edit Markdown directly in live preview: reveal active/selected source lines,
  retain surrounding formatting, map clicks to source, and support normal editing
  keys including Shift+Home/End. Cache parsed Markdown by buffer revision.
- Request enhanced modified-key reporting from supporting terminals.

- Remove the fixed 25 ms Git polling floor and avoid redraws for unchanged Git
  snapshots. Log aggregate input/render timings without recording keys or text.

- Refresh Git diffs when HEAD moves, show staged changes before the first commit,
  isolate deletion markers by file, and capture Git output in anonymous temporary files.

- Reject stale or invalid language-server edit batches before changing any buffer.
  Synchronize all matching open documents, preserve pending edits across tab switches,
  handle close/reopen, and use UTF-16 positions and escaped file URIs.

- Cache syntax parser checkpoints and visible styles, yielding between chunks
  when scrolling deep into a file. Very long lines omit syntax colors.
- Cache search matches by buffer revision and correctly map expanding Unicode
  lowercase matches back to source positions. Limit unwrapped rendering to the viewport.

- Make external reloads undoable, retain earlier editing history and line endings,
  and preserve cursor/selection context and viewport when text shifts.
- Retry reads that change during loading and defer external prompts while another
  editing dialog is active.

- Preserve file permissions and symlinks when saving; use unique, synced temporary
  files and recheck disk changes before replacement. Save As confirms overwrites.
- Reject overlapping range edits and fix overlapping case-insensitive Replace All.
- Keep safety dialogs ahead of custom shortcuts and restore the terminal after
  partial initialization failures. Declare Rust 1.88 as the minimum compiler.

- Focus TTED on terminal editing; remove integrated chat, agent API, approvals,
  and agent-specific change tracking. External tools continue to edit files directly.
- Remove agent commands and bindings; legacy agent configuration is ignored.

## 0.1.1 - 2026-08-20

- Make task-list checkboxes clickable and undoable in Markdown reading view.

## 0.1.0 - 2026-08-19

- Conventional Unicode-aware rope editing, mouse selection, tabs, explorer,
  quick open, search/replace, syntax highlighting, and Markdown reader.
- Catppuccin-inspired terminal UI, Focus Mode, split views, command palette,
  and a persistent mouse/keyboard Keybindings menu.
- Git status, decorations, diffs, and safe file-level write operations.
- Configured LSP lifecycle, diagnostics, navigation, completion, and edits.
- Permission-scoped structured agent API and integrated agent panel.
- Zero-friction built-in Codex chat using the managed app-server protocol.
- Scrollable, role-labeled Agent chat; native Unicode positional edits; and a
  nested-container-safe workspace sandbox for routine Codex collaboration.
- TOML configuration for editing, keybindings, explorer, LSP, and agent access.
- Directory/file CLI workflow, CI, release packaging, and installer.
- Checksummed release downloads, source-build fallback, automatic user PATH
  setup, and distribution-specific Linux installation documentation.
