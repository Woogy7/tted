# TTED

TTED (Terminal Text Editor) is an early-stage, conventional terminal text editor.
It is designed to work naturally in ordinary terminals, SSH sessions, tmux, and
Herdr without requiring an AI agent.

## Current v0.1

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
- background Git branch/clean-state detection and explorer file decorations
- Git added/modified/deleted line markers in the editor gutter
- read-only Git status, current-file diff, and workspace-diff tabs

- configured language-server diagnostics, navigation, completion, and edits
- two-pane split editing
- a permission-scoped structured agent API and optional integrated Agent panel
- optional TOML configuration for editing, keybindings, explorer, LSP, and agents

## Build and run

```sh
cargo build
cargo run -- .
cargo run -- README.md
cargo run -- file1.rs file2.rs
```

With no filenames TTED opens an untitled buffer; Ctrl+S then prompts for a path.
Passing a directory selects that workspace and opens its explorer.

For released builds, download a platform archive or run:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/Woogy7/tted/main/install.sh | sh
```

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
| Ctrl+F | Open Find and Replace |
| Ctrl+P | Fuzzy-find and open a workspace file |
| F2 / Ctrl+Shift+P | Open the Command Palette |
| F3 | View and change keybindings |
| Ctrl+W | Close tab; press twice when it has unsaved changes |
| Ctrl+E | Toggle and focus the file explorer |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+Tab / Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab / Ctrl+PageUp | Previous tab |
| Alt+Left / Alt+Right | Previous / next tab (portable fallback) |
| Ctrl+Shift+M / F6 | Toggle Markdown source / reading view |
| Ctrl+G / F9 | Open or close the built-in Agent chat |
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

Ctrl+F opens centered Find and Replace. Enter and Shift+Enter move to the next
and previous matches, Tab switches fields, Alt+C toggles case sensitivity,
Ctrl+R replaces the current match, and Ctrl+Shift+R replaces all matches.

F2 opens the Command Palette reliably in every terminal. Ctrl+Shift+P also works
when the terminal distinguishes it from Ctrl+P. Commands can be found by fuzzy title or
stable ID, then run with Enter. Right-click an explorer item to surface its file
operations without memorizing the explorer keys.

The Command Palette also provides `Git: Open Status`, `Git: Open Current File
Diff`, and `Git: Open Workspace Diff`. These views open as read-only tabs; close
them normally with Ctrl+W.

File-level Git actions are also available from the Command Palette: stage or
unstage the current saved file, discard its tracked changes after confirmation,
and commit staged changes with a message. Git operations run in the background.
TTED deliberately does not delete untracked files or provide push/pull actions.

Inside a Git repository, the status bar shows the branch plus `✓` for clean or
`*` for dirty. Explorer files use subtle `M`, `A`, `?`, and `D` decorations.
The line-number gutter marks added lines in green, modified lines in peach, and
deletion points in red.

TTED writes lightweight diagnostic and Git-worker timing logs to
`/tmp/tted-<pid>.log`. Set `TTED_LOG=/path/to/file.log` to choose another path.

Language servers are configured by extension in `.tted.toml`; copy
`.tted.example.toml` as a starting point. Diagnostics appear in the gutter,
explorer, and the collapsible Problems panel. Hover, definition, completion,
references, rename, formatting, symbols, code actions, signature help, and
server restart are available from the Command Palette. F8 opens Problems and
jumps through diagnostics.

The Command Palette can split the editor right or down, focus the adjacent
split, and close the split. Panes reference the same underlying open buffers;
clicking an inactive pane focuses it.

An optional structured agent API listens on `/tmp/tted-<pid>.sock` by default.
It uses stable buffer IDs and revision-checked JSON-RPC edits rather than
terminal scraping or simulated keys. Mutation permissions default off; see
`AGENT_API.md` and `.tted.example.toml`.

F9 opens a collapsible, conventional Agent chat. TTED automatically detects an
installed Codex CLI, reuses its sign-in, or presents a clickable device-code
setup. Type a request and press Enter; Shift+Enter inserts a newline. Responses,
commands, edits, completion, and errors stream into the panel. Stop, Retry, New,
Clear, Diff, Accept, and Revert are clickable. Codex is confined to the current
workspace, and Revert refuses to overwrite later human changes. The existing
provider-neutral socket API remains available for advanced/custom backends.

When an open file changes on disk, TTED reloads it automatically if the editor
buffer is clean. If unsaved edits could be lost, use `R` to reload the disk
version or `K` to keep the editor version. Deleted files can be kept and recreated
with Save.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Documentation: [keybindings](KEYBINDINGS.md), [configuration](CONFIGURATION.md),
[architecture](ARCHITECTURE.md), [Agent chat](AGENT_CHAT.md),
[agent API](AGENT_API.md),
[contributing](CONTRIBUTING.md), and [changelog](CHANGELOG.md).
