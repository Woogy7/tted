# TTED project status

This document summarizes the Terminal Text Editor implemented so far. TTED is a
standalone terminal editor: it does not require an AI agent and is designed to
behave naturally in ordinary terminals, SSH sessions, tmux, and Herdr.

## Current product

TTED is a working Rust terminal application that can open several UTF-8 files,
display them in tabs, edit them with conventional keyboard and mouse controls,
and save them safely. It enters raw mode and the alternate screen while running
and restores terminal state on normal shutdown.

The current interface includes:

- a clickable tab bar with dirty-file indicators;
- a Catppuccin Mocha-inspired color palette;
- file-aware syntax highlighting;
- line numbers and a status bar;
- a collapsible workspace file explorer;
- a keybindings popup;
- editable Markdown source and a separate formatted reading view.

## Editing behavior

Text is stored in a Ropey UTF-8 rope. Buffers support insertion, selections,
replacement, Backspace, Delete, multiline bracketed paste, undo, and redo. Edit
history and cursor state belong to each buffer.

Navigation supports arrows, Home, End, Page Up, Page Down, Shift-selection,
mouse clicks, mouse dragging, vertical scrolling, and horizontal scrolling.
Movement, deletion, rendering, and mouse hit-testing understand Unicode grapheme
clusters and terminal cell widths. Combined emoji move and delete as one unit,
and wide characters preserve their visual column during vertical movement.

Indentation conveniences currently include:

- Enter retaining the current line's leading indentation;
- an extra indentation level after `{`, `[`, or `(`;
- an indented blank line when Enter is pressed between a bracket pair;
- Tab advancing to four-column stops;
- Backspace removing leading spaces to the previous tab stop;
- Shift+Tab unindenting the current line.

TTED maintains an internal clipboard for Ctrl+C, Ctrl+X, and Ctrl+V. Copy and cut
also emit OSC 52 so compatible local terminals, tmux configurations, and SSH
sessions can update the host clipboard. Native terminal paste is accepted as one
bracketed-paste edit.

## Files and workspace

Files supplied on the command line open as tabs. Tabs can be selected by mouse,
cycled by keyboard, and closed with unsaved-change protection. Save uses a
temporary sibling file followed by rename. Save As supports both existing and
untitled buffers, and successful Save As updates the tab filename.

The file explorer is toggled with Ctrl+B. It lists ordinary workspace files by
relative path, omits hidden folders and `target`, and opens files by mouse click.
Clicking a file that is already open focuses its existing tab. The explorer
automatically stays hidden when the terminal is too narrow.

## Search, syntax, and Markdown

Ctrl+F opens an incremental input prompt, then selects the next exact match and
wraps at the end of the file.

Syntax highlighting is selected by filename and currently covers the common
programming and markup formats in the playground, including Rust, Ruby, Python,
JavaScript, HTML, CSS, JSON, and Markdown.

Markdown files can switch between editable source and a read-only reading view
with Ctrl+Shift+M or F6. The reading view formats headings, paragraphs, lists,
task markers, emphasis, quotes, rules, inline code, code blocks, and raw HTML.
Global commands such as Ctrl+Q, Ctrl+W, Ctrl+B, and tab switching remain available
from the reading view.

## Key controls

| Key | Action |
|---|---|
| F1 | Show or close keybindings help |
| Arrows, Home, End, Page Up/Down | Navigate |
| Shift + navigation | Select |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, paste |
| Ctrl+Z / Ctrl+Y | Undo, redo |
| Ctrl+S / Ctrl+Shift+S | Save, Save As |
| Ctrl+F | Find text |
| Ctrl+W | Close the current tab |
| Ctrl+B | Toggle the file explorer |
| Alt+Left / Alt+Right | Previous or next tab |
| Ctrl+Shift+M / F6 | Toggle Markdown reading view |
| Ctrl+Q | Quit, with unsaved-change protection |

Ctrl+Tab is handled when the terminal can distinguish it from Tab. Alt+Left and
Alt+Right are the portable tab-switching fallback.

## Code organization

TTED remains a single Rust crate with deliberately small modules:

- `buffer.rs` owns text, cursor and selection state, revisions, history, Unicode
  coordinate conversion, searching, and persistence;
- `editor.rs` owns application state, event dispatch, tabs, panels, prompts,
  terminal rendering, syntax styles, and workspace interactions;
- `markdown.rs` converts parsed Markdown events into styled terminal lines;
- `theme.rs` defines the shared Catppuccin-inspired interface palette;
- `main.rs` owns terminal setup and guaranteed teardown.

All editor mutations run through the single UI event loop. Background async work,
LSP, Git services, and the future agent API have not been introduced yet.

## Verification

The project currently has 13 passing unit tests covering buffer edits, undo/redo,
selection replacement, indentation, wrapped search, Save As, Markdown rendering,
file-explorer filtering, raw terminal control keys, Markdown-view quitting, and
Unicode grapheme behavior.

The development checks are:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Interactive pseudo-terminal checks have also covered startup, syntax rendering,
the help popup, explorer toggling, Markdown reading mode, Ctrl+Q, and restoration
of mouse, focus, bracketed-paste, cursor, raw, and alternate-screen modes.

## Known limitations and next work

This is still an early editor foundation. Notable limitations are:

- undo currently stores full rope snapshots and typing is not yet coalesced into
  natural undo groups;
- the explorer is a flattened, mouse-oriented list without directory expansion,
  keyboard focus, scrolling, file creation, rename, or deletion;
- search is exact and case-sensitive, with no live match count or replacement;
- syntax state is recomputed for visible rendering rather than cached by buffer
  revision;
- external file changes are not detected;
- clipboard paste from outside TTED depends on the terminal's native paste path;
- configuration, custom keybindings, Git information, LSP, split panes, and the
  agent interaction API remain to be built.

The recommended next phase is reliability and workspace depth: detect external
file changes, turn the explorer into a keyboard-navigable collapsible tree, and
improve undo transaction grouping. Git status can follow, then the asynchronous
service boundary needed for LSP.
