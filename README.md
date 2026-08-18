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
- a collapsible, mouse-enabled workspace file explorer
- grapheme-aware movement and deletion with terminal-cell-aware cursor placement

Syntax highlighting, the file explorer, Markdown rendering, Git, LSP, and the
agent API are intentionally deferred until the editing core is proven.

## Build and run

```sh
cargo build
cargo run -- path/to/file another/file
```

With no filenames TTED opens an untitled buffer. Saving an untitled buffer is not
yet supported.

## Keys

| Key | Action |
|---|---|
| Arrow keys, Home, End | Move cursor |
| Page Up / Page Down | Move one screen |
| Shift + navigation | Select text |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, or paste the TTED clipboard |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save As |
| Ctrl+F | Find text in the current file |
| Ctrl+W | Close tab; press twice when it has unsaved changes |
| Ctrl+B | Toggle the file explorer |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+Tab / Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab / Ctrl+PageUp | Previous tab |
| Alt+Left / Alt+Right | Previous / next tab (portable fallback) |
| Ctrl+Shift+M / F6 | Toggle Markdown source / reading view |
| Ctrl+Q | Quit; press twice when changes are unsaved |
| F1 | Show keybindings help |

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
