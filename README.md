# TTED

TTED (Terminal Text Editor) is an early-stage, conventional terminal text editor.
It is designed to work naturally in ordinary terminals, SSH sessions, tmux, and
Herdr without requiring an AI agent.

## Current v0.1 foundation

- UTF-8 text buffers backed by a rope
- multiple files displayed as tabs
- arrow, Home/End, and Page Up/Down navigation
- Shift-selection and mouse positioning/drag selection
- insertion, deletion, bracketed paste, undo, and redo
- vertical and horizontal scrolling
- LF/CRLF preservation and atomic saves
- dirty indicators and protection against accidentally quitting with changes
- reliable terminal cleanup through an RAII guard
- file-aware syntax highlighting for common programming and markup languages
- a read-only Markdown reading view toggled independently from source editing
- a Catppuccin Mocha-inspired editor interface
- a collapsible workspace tree with keyboard and mouse navigation
- grapheme-aware movement and deletion with terminal-cell-aware cursor placement
- natural undo groups for typing and deletion, with atomic paste and indentation
- smart closing-bracket dedent that aligns closers with their opening level
- automatic clean-file reload and explicit reload/keep prompts for disk conflicts

Git, LSP, and the agent API remain deferred while the workspace experience is
developed further.

## Build and run

```sh
cargo build
cargo run -- path/to/file another/file
```

With no filenames TTED opens an untitled buffer; Ctrl+S then prompts for a path.

## Keys

| Key | Action |
|---|---|
| Arrow keys, Home, End | Move cursor |
| Page Up / Page Down | Move one screen |
| Shift + navigation | Select text |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, or paste the TTED clipboard |
| Ctrl+S | Save |
| Ctrl+N | Create and open a new workspace file |
| Ctrl+Shift+S | Save As |
| Ctrl+F | Find text in the current file |
| Ctrl+P | Fuzzy-find and open a workspace file |
| Ctrl+W | Close tab; press twice when it has unsaved changes |
| Ctrl+E | Toggle and focus the file explorer |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+Tab / Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab / Ctrl+PageUp | Previous tab |
| Alt+Left / Alt+Right | Previous / next tab (portable fallback) |
| Ctrl+Shift+M / F6 | Toggle Markdown source / reading view |
| F11 | Toggle document-only Focus Mode |
| Ctrl+Q | Quit; press twice when changes are unsaved |
| F1 | Show keybindings help |

In the explorer, use arrows, Home/End, or Page Up/Down to navigate; Left/Right
collapse or expand folders, Enter opens a file, and Esc or Tab returns focus to
the document. N creates a file, Shift+N creates a directory, R renames, and D
opens a permanent-delete confirmation.

New-file and rename input appears in a centered dialog. Successfully creating a
file opens its tab immediately with document focus. Tabs keep the active file in
view when they overflow and include a clickable `×` close control.

Closing a dirty tab opens a centered confirmation dialog; Y discards and closes,
while N or Esc returns safely to the document. F11 hides tabs, status, explorer,
and other chrome, then restores the previous layout when pressed again.

Ctrl+P opens a centered Quick Open picker. Type any ordered fragments of a
filename or relative path, navigate matches with arrows or Page Up/Down, and
press Enter to open the selection.

When an open file changes on disk, TTED reloads it automatically if the editor
buffer is clean. If unsaved edits could be lost, use `R` to reload the disk
version or `K` to keep the editor version. Deleted files can be kept and recreated
with Save.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
