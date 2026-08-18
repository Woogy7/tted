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

Sequential typing and deletion are grouped into natural undo transactions.
Navigation ends the active group, while paste, selection replacement, newline,
and indentation changes remain deliberate atomic transactions. Undo and redo
track saved-content identity, so returning to the saved text clears the dirty
indicator correctly.

Navigation supports arrows, Home, End, Page Up, Page Down, Shift-selection,
mouse clicks, mouse dragging, vertical scrolling, and horizontal scrolling.
Movement, deletion, rendering, and mouse hit-testing understand Unicode grapheme
clusters and terminal cell widths. Combined emoji move and delete as one unit,
and wide characters preserve their visual column during vertical movement.

Indentation conveniences currently include:

- Enter retaining the current line's leading indentation;
- an extra indentation level after `{`, `[`, or `(`;
- an indented blank line when Enter is pressed between a bracket pair;
- a closing bracket typed on an indentation-only line moving back one level;
- Tab advancing to four-column stops;
- Backspace removing leading spaces to the previous tab stop;
- Shift+Tab unindenting the current line.

TTED maintains an internal clipboard for Ctrl+C, Ctrl+X, and Ctrl+V. Copy and cut
also emit OSC 52 so compatible local terminals, tmux configurations, and SSH
sessions can update the host clipboard. Native terminal paste is accepted as one
bracketed-paste edit.

## Files and workspace

Files supplied on the command line open as tabs. Tabs can be selected by mouse,
cycled by keyboard, and closed by Ctrl+W or their mouse `×`, with a centered
unsaved-change confirmation. When tabs overflow, the visible window follows the
active tab. Save uses a
temporary sibling file followed by rename. Save As supports both existing and
untitled buffers, and successful Save As updates the tab filename and refreshes
the explorer immediately.

The file explorer is toggled with Ctrl+E, avoiding the default prefix used by
tmux and Herdr. It presents a lazy directory tree,
omits hidden and common build directories, and opens focused files with Enter or
a mouse click. Arrows, Home/End, Page Up/Down, and the mouse wheel navigate it;
Left/Right and clicks collapse or expand folders. Esc or Tab returns focus to
the document. Opening a file that is already open focuses its existing tab. The
explorer automatically stays hidden when the terminal is too narrow.

With the explorer focused, N creates a file, Shift+N creates a directory, R
renames the selected item, and D requests deletion. Delete requires explicit Y
confirmation. Operations reject path traversal and accidental overwrite; open
files and folders containing open files must be closed before rename or delete.
Ctrl+N opens the centered new-file dialog from anywhere; a created file opens
immediately with focus ready for editing.

Open files are checked periodically for disk changes. Clean buffers reload
automatically. A changed file with unsaved editor content requires an explicit
reload-or-keep decision, and deleted files can be kept in memory and recreated
with Save. Mouse, paste, and ordinary editing input are blocked until conflicts
are resolved.

## Search, syntax, and Markdown

Ctrl+F opens a centered find-and-replace dialog. It selects matches as the query
changes, shows the current position and total count, wraps next/previous
navigation, and supports a case-sensitive toggle. Replace-current advances to
the next result; replace-all is one undoable transaction.

Ctrl+P opens a centered Quick Open picker over workspace files. It performs
case-insensitive fuzzy path matching, supports arrow and page navigation, and
opens or focuses the selected file with Enter.

Syntax highlighting is selected by filename and currently covers the common
programming and markup formats in the playground, including Rust, Ruby, Python,
JavaScript, HTML, CSS, JSON, and Markdown.

Markdown files can switch between editable source and a read-only reading view
with Ctrl+Shift+M or F6. The reading view formats headings, paragraphs, lists,
task markers, emphasis, quotes, rules, inline code, code blocks, and raw HTML.
Arrow keys, Page Up/Down, Home/End, and the mouse wheel scroll rendered content.
Global commands such as Ctrl+Q, Ctrl+W, Ctrl+E, and tab switching remain available
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
| Ctrl+F | Find and replace text |
| Ctrl+P | Fuzzy-find and open a workspace file |
| Ctrl+W | Close the current tab |
| Ctrl+E | Toggle and focus the file explorer |
| Ctrl+N | Create and open a new workspace file |
| Alt+Left / Alt+Right | Previous or next tab |
| Ctrl+Shift+M / F6 | Toggle Markdown reading view |
| F11 | Toggle document-only Focus Mode |
| Ctrl+Q | Quit, with unsaved-change protection |

Ctrl+Tab is handled when the terminal can distinguish it from Tab. Alt+Left and
Alt+Right are the portable tab-switching fallback.

## Code organization

TTED remains a single Rust crate with deliberately small modules:

- `buffer.rs` owns text, cursor and selection state, revisions, history, Unicode
  coordinate conversion, searching, and persistence;
- `editor.rs` owns application state, event dispatch, tabs, panels, prompts,
  terminal rendering, syntax styles, and workspace interactions;
- `explorer.rs` owns the lazy workspace tree, selection, scrolling, and folder
  expansion state;
- `markdown.rs` converts parsed Markdown events into styled terminal lines;
- `theme.rs` defines the shared Catppuccin-inspired interface palette;
- `main.rs` owns terminal setup and guaranteed teardown.

All editor mutations run through the single UI event loop. Background async work,
LSP, Git services, and the future agent API have not been introduced yet.

## Verification

The project currently has 44 passing unit tests covering buffer edits, natural
undo groups, saved/dirty identity, multiline selection and paste, CRLF and final
newline preservation, wrapped search, Save As, external modification/deletion
flows, Markdown rendering, file-explorer filtering, raw terminal control keys,
Markdown-view quitting, Unicode grapheme behavior, long lines, and large files.

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

- undo transactions still store full rope snapshots; grouping is natural now,
  but a compact edit-based history may eventually reduce memory use;
- explorer filtering is not yet gitignore-aware;
- workspace-wide content search is not yet available;
- syntax state is recomputed for visible rendering rather than cached by buffer
  revision;
- file-change detection currently polls file metadata rather than using a
  background filesystem watcher;
- clipboard paste from outside TTED depends on the terminal's native paste path;
- configuration, custom keybindings, Git information, LSP, split panes, and the
  agent interaction API remain to be built.

Phase 1 editing-core hardening and Phase 2's workspace, explorer, tabs, Focus
Mode, Quick Open, and search workflows are complete. The next roadmap phase is
Git integration.
