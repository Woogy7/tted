# TTED Development Roadmap

TTED — **Terminal Text Editor**, pronounced “Ted” — is a modern, intuitive, mouse-friendly and agent-native terminal text editor.

The long-term goal is:

> **Neovim-quality terminal behaviour, VS Code/Sublime-style usability, and Herdr-level mouse-native and agent-native interaction.**

TTED is an **editor**, not a terminal multiplexer and not an agent harness.

It must work naturally:
- in a normal terminal;
- over SSH;
- inside tmux;
- inside Herdr.

AI/agent functionality must always remain optional. TTED must be an excellent editor when no agent is connected.

---

# Core Product Principles

These principles are architectural constraints, not optional features.

## 1. Conventional editing

TTED should be immediately understandable to someone familiar with Sublime Text, VS Code, Notepad++, or ordinary GUI editors.

Normal typing should type text.

Do not require modal editing.

Standard navigation should work naturally:

- Arrow keys
- Shift + arrows for selection
- Home / End
- Page Up / Page Down
- Ctrl + arrows where terminal support allows it
- Mouse click positioning
- Mouse drag selection
- Scroll wheel

## 2. Mouse-native and keyboard-native

Anything obvious in the UI should generally be clickable.

Anything important should also have a keyboard path.

Neither mouse nor keyboard interaction should feel secondary.

## 3. Document-first UI

Sidebars and panels are tools, not the application.

TTED must eventually support:

- Explorer sidebar
- Editor tabs
- Bottom/tool panels
- Agent area
- Problems/diagnostics
- Git information
- Command palette

Every non-editor panel must be collapsible.

There must be a **Focus Mode** that hides all optional UI and leaves essentially only the current document.

## 4. Excellent terminal citizenship

TTED should behave as cleanly inside tmux or Herdr as Neovim does.

It must:

- resize correctly;
- restore terminal state reliably;
- handle mouse events correctly;
- avoid unnecessary full-screen redraws;
- work well over SSH;
- handle Unicode correctly;
- preserve terminal clipboard interoperability;
- never attempt to become a terminal multiplexer.

## 5. Agent-native, not agent-dependent

Agents should eventually interact with TTED through structured editor operations.

Agents should not have to simulate keyboard presses or scrape rendered terminal text.

TTED itself remains useful without any agent.

## 6. Keep the architecture understandable

This is both a real project and a learning project.

Prefer:

- clear Rust;
- small modules;
- explicit state;
- well-defined interfaces;
- tests for important behaviour;
- few dependencies;
- incremental architectural changes.

Avoid speculative abstractions and premature plugin systems.

---

# Current State — v0.1 Foundation

The project already has a substantial editing foundation.

Implemented functionality includes:

- Ropey-backed UTF-8 buffers
- Multiple open files/tabs
- Conventional cursor navigation
- Shift-selection
- Mouse positioning
- Mouse drag selection
- Vertical and horizontal scrolling
- Unicode grapheme-aware editing
- Terminal-cell-aware cursor placement
- Insert/delete
- Multiline bracketed paste
- Undo/redo
- Basic indentation assistance
- Internal clipboard
- OSC 52 copy support
- Atomic saves
- Save As
- Untitled buffer Save As
- Dirty indicators
- Unsaved-change protection
- Syntax highlighting
- Line numbers
- Status bar
- Catppuccin-inspired theme
- Collapsible file explorer
- Incremental exact search
- Markdown source editing
- Markdown formatted reading view
- Keybindings help
- Reliable terminal setup/cleanup
- Unit tests and interactive terminal checks

Current architecture intentionally remains small:

- `buffer.rs`
- `editor.rs`
- `markdown.rs`
- `theme.rs`
- `main.rs`

Do not perform a major architectural rewrite simply for cleanliness.

Refactor only when upcoming functionality creates a concrete need.

---

# Phase 1 — Finish and Harden the Editing Core

**Status: ✅ Complete**

Phase 1 was completed with natural time-bounded typing/deletion transactions,
saved-content-aware dirty tracking, periodic external file detection, automatic
clean reloads, explicit reload/keep conflict handling, expanded reliability
coverage, and large-file sanity tests. Undo still uses rope snapshots by design;
memory compaction remains a later optimization if measurements justify it.

## Goal

Make the basic editing experience trustworthy enough that TTED can genuinely be used to edit real projects.

This phase comes before Git, LSP, or agents.

## 1.1 Natural undo transactions

**✅ Completed.** Sequential typing, Backspace, and Delete coalesce naturally.
Movement breaks a group. Paste, indentation, newline insertion, and selection
replacement remain atomic.

Current undo uses full rope snapshots.

Improve undo behaviour so normal typing feels like an editor rather than every character being an isolated action.

Examples:

Typing:

```text
hello world
```

should normally undo as a sensible typing transaction rather than requiring eleven Ctrl+Z presses.

Define sensible grouping for:

- sequential typing;
- deletion;
- paste;
- indentation;
- selection replacement;
- newline insertion.

Preserve correctness before optimizing memory representation.

A more compact edit-history representation can come later if necessary.

## 1.2 External file change detection

**✅ Completed.** TTED polls open-file metadata, automatically reloads clean
buffers, prompts before replacing dirty buffers, detects deletion and recreation,
and supports keeping deleted content so Save recreates the file.

Detect when an open file changes on disk.

Handle at minimum:

- file changed externally while buffer is clean;
- file changed externally while buffer contains unsaved edits;
- file deleted externally.

Never silently destroy unsaved editor content.

Provide a simple, understandable reload/keep-editing flow.

## 1.3 Editing reliability

**✅ Completed.** Tests now cover Unicode graphemes and cell widths, multiline
selection and paste, grouped undo/redo, saved-state identity, CRLF preservation,
files without a final newline, Save As, external changes, long lines, and deleted
files.

Expand tests around:

- Unicode;
- multiline editing;
- selections;
- paste;
- undo/redo;
- CRLF files;
- Save As;
- external file changes;
- very long lines;
- files without final newline.

## 1.4 Performance sanity checks

**✅ Completed.** Debug tests exercise editing a roughly 1.6 MB/20,000-line buffer,
a 200,000-character line, and first syntax render of a 20,000-line Rust file. The
complete 28-test suite runs well below the deliberately generous five-second
sanity thresholds in the development environment. Grapheme navigation was also
changed to inspect only the current line instead of cloning the full buffer.

Test reasonably large files and ensure:

- scrolling remains responsive;
- editing remains responsive;
- syntax rendering does not introduce obvious latency;
- memory use is reasonable.

Do not prematurely optimize without measurements.

## Exit criteria

TTED can reliably be used for ordinary text editing without fear of losing data or corrupting terminal state.

---

# Phase 2 — Workspace and Explorer UX

## Goal

Turn TTED from a capable file editor into a pleasant project editor.

## 2.1 Real tree explorer

Replace the flattened explorer with a directory tree.

**✅ Completed.** The explorer now builds a lazy, directories-first tree with
indentation and exclusions for hidden/build directories. It supports keyboard
focus, arrow/Home/End/Page navigation, scrolling, Left/Right collapse and
expand, Enter to open, and equivalent mouse click/wheel interactions.

Support:

- folders;
- expand/collapse;
- indentation;
- scroll;
- keyboard focus;
- arrow-key navigation;
- Enter to open;
- mouse open;
- expand/collapse by mouse.

Avoid automatically scanning huge ignored directories.

Respect sensible exclusions such as `.git` and build directories.

Gitignore-aware filtering can be added where appropriate.

## 2.2 Explorer file operations

Add deliberate file operations:

**✅ Completed.** With the explorer focused, `N`, `Shift+N`, `R`, and `D` create
files, create directories, rename, and request deletion. Operations use a
single-name prompt, refuse overwrite and workspace-escape paths, refresh the
tree on success, and require explicit confirmation before recursive deletion.
Open files and folders containing them are protected from rename or deletion.

- New file
- New directory
- Rename
- Delete

Destructive operations must require an appropriate confirmation.

## 2.3 Better tabs

Improve the existing tab bar.

**✅ Completed for the required v0.1 scope.** Tabs retain clear active and dirty
styling, expose a mouse close affordance with unsaved-change protection, and
shift their visible window automatically so the active tab remains visible when
the bar overflows. Drag/reordering remains optional future work.

Eventually support:

- click to activate;
- mouse close affordance;
- dirty indicator;
- horizontal tab scrolling when necessary;
- clear active-tab styling.

Tab drag/reordering is desirable but not required immediately.

## 2.4 Focus Mode

Implement a one-action Focus Mode.

**✅ Completed.** `F11` toggles a document-only layout that hides the explorer,
tabs, status bar, and other chrome. Exiting restores whether the explorer was
visible before Focus Mode was entered.

Focus Mode should hide:

- Explorer
- auxiliary panels
- unnecessary chrome

and maximize the document area.

Restoring Focus Mode must return the previous layout.

## 2.5 Quick Open

Add a fast file picker similar in spirit to:

```text
Ctrl+P
```

It should allow fuzzy filename/path search within the workspace.

This becomes one of the most important keyboard workflows.

## 2.6 Search improvements

Expand search to support:

- next match;
- previous match;
- match count;
- case-sensitive toggle;
- replacement;
- replace all.

Workspace-wide search can follow once the file/project model is solid.

## Exit criteria

Opening and navigating a multi-file project should feel natural using either mouse or keyboard.

---

# Phase 3 — Command System and Discoverability

## Goal

Create one coherent action system before adding many advanced features.

## 3.1 Internal commands

Represent editor actions as named commands rather than scattering behaviour across keyboard event handlers.

Examples:

```text
file.open
file.save
file.save_as
file.close
workspace.quick_open
workspace.toggle_explorer
view.focus_mode
editor.find
editor.replace
tab.next
tab.previous
markdown.toggle_reader
```

Keyboard shortcuts, menus, the command palette, and eventually agents should invoke the same command system.

## 3.2 Command Palette

Add a command palette, likely:

```text
Ctrl+Shift+P
```

Example:

```text
> File: Save As
> View: Toggle Explorer
> View: Focus Mode
> Markdown: Toggle Reader
> Git: Show Status
```

This is central to TTED's discoverability philosophy.

## 3.3 Context actions

Allow obvious mouse interactions to surface available operations.

Do not create complicated nested UI prematurely.

## Exit criteria

Users should be able to discover most major TTED operations without reading documentation.

---

# Phase 4 — Git Awareness

## Goal

Provide useful Git information without trying to recreate a full Git client.

Start read-only.

## 4.1 Repository detection

Detect whether the workspace belongs to a Git repository.

Display:

- current branch;
- clean/dirty repository state.

## 4.2 Explorer decorations

Show file state where useful:

```text
M modified
A added
? untracked
D deleted
```

Keep decorations visually subtle.

## 4.3 Editor gutter changes

Show changed lines where practical:

- added;
- modified;
- deleted.

## 4.4 Git status / diff view

Provide simple views for:

- repository status;
- current file diff;
- workspace diff.

Prefer reusing TTED's normal buffer/view system.

## 4.5 Write operations

Only after read-only Git functionality is reliable, consider:

- stage;
- unstage;
- discard selected changes;
- commit.

Do not implement push/pull/branch-management complexity at this stage.

## Exit criteria

A developer can understand what has changed in the repository without leaving TTED.

---

# Phase 5 — Background Service Architecture

## Goal

Introduce asynchronous work cleanly before LSP and agents require it.

Currently editor state mutations run through a single UI event loop. Preserve that useful property.

Background services should communicate results back to the editor rather than mutate UI/editor state directly.

Design a small service/event boundary supporting:

- background jobs;
- events/messages;
- cancellation;
- child processes;
- streamed output;
- errors;
- graceful shutdown.

Potential future services include:

```text
GitService
LspService
FileWatcher
WorkspaceIndexer
AgentService
```

Do not create a large framework.

Implement only what upcoming features require.

## Exit criteria

TTED can perform background work without blocking typing, scrolling, or rendering.

---

# Phase 6 — LSP

## Goal

Give TTED modern code intelligence while keeping the editor itself language-agnostic.

Start with a small, useful LSP implementation.

## 6.1 Language server lifecycle

Support:

- detect configured language;
- start language server;
- initialize;
- open/change/save notifications;
- graceful shutdown;
- restart after failure where appropriate.

## 6.2 Diagnostics

First major visible feature.

Show:

- errors;
- warnings;
- file decorations;
- line/gutter indicators;
- Problems panel.

Mouse and keyboard navigation should jump directly to diagnostics.

## 6.3 Hover

Provide symbol/type/documentation information without obstructing normal editing.

## 6.4 Go to definition

Support familiar code navigation.

Maintain navigation history so a user can return easily.

## 6.5 Completion

Add completion carefully.

Typing responsiveness must always take priority.

## 6.6 Later LSP features

After the basics are reliable:

- references;
- rename symbol;
- code actions;
- formatting;
- document symbols;
- workspace symbols;
- signature help.

## Exit criteria

TTED is genuinely comfortable for editing code in an LSP-supported language.

---

# Phase 7 — Views, Buffers and Split Editing

## Goal

Support richer editor layouts without becoming a window manager.

Ensure the internal model clearly distinguishes:

```text
File
Buffer
View
Tab
Pane
```

A buffer may eventually be visible in more than one view.

## 7.1 Split views

Support:

- Split Right
- Split Down
- Close Split
- Focus adjacent split

Mouse resizing is desirable.

Keep layouts intentionally simple.

## 7.2 Special views

Reuse the view system where sensible for:

- Markdown reader
- Git diff
- Problems
- help
- search results

Avoid creating a separate rendering architecture for every feature.

## Exit criteria

TTED supports useful editor-level splitting while remaining a good citizen inside tmux and Herdr.

---

# Phase 8 — Agent-Native Core

## Goal

Make TTED unusually good for coding agents without turning TTED into an agent harness.

The critical architectural rule is:

> **Agents interact with structured editor state, not terminal pixels or simulated keystrokes.**

## 8.1 Agent-safe editor API

Expose editor operations such as:

```text
workspace.info
workspace.files

buffer.list
buffer.read
buffer.revision

editor.current_file
editor.cursor
editor.selection
editor.open
editor.focus_range

edit.apply
edit.apply_batch

diagnostics.list

git.status
git.diff
```

Use stable identifiers and revisions where necessary so an agent cannot accidentally apply an edit against stale content.

## 8.2 One command system

Agent actions should use the same underlying editor command/edit systems as human UI actions wherever appropriate.

Do not build two separate editors:

- one for humans;
- one for agents.

## 8.3 Transport

Provide a structured local interface.

A Unix-domain socket with a simple structured protocol such as JSON-RPC is a reasonable starting direction, but evaluate the simplest robust solution before committing.

The transport should allow an external agent process to interact directly with a running TTED instance.

## 8.4 Permissions

Agent access must be visible and controllable.

Possible capabilities:

```text
Read workspace
Read editor state
Read diagnostics
Read Git state
Modify buffers
Create files
Delete files
Run commands
```

Do not give an agent unrestricted capabilities merely because it connected.

## 8.5 Visible agent activity

TTED should show when an agent is:

- reading;
- editing;
- waiting;
- running an action;
- proposing changes;
- encountering an error.

The goal is Herdr-style visibility of agent state.

## Exit criteria

An external coding agent can inspect and edit a running TTED session through a structured interface without pretending to be a human keyboard user.

---

# Phase 9 — Integrated Agent Area

## Goal

Add the human-facing agent experience after the editor API is solid.

The agent area is an editor panel, not the foundation of the application.

Possible layout:

```text
Agent
────────────────────────────────

> Refactor this function and add tests

● Reading customer.rs
● Modified customer.rs
● Modified customer_test.rs
✓ Tests passed

[View Diff] [Accept] [Revert]
```

Support:

- prompt input;
- streamed agent responses;
- visible tool/action activity;
- current task state;
- clickable file references;
- diff inspection;
- approval flows where appropriate.

Eventually support actions from context:

```text
Ask Agent
Explain Selection
Refactor Selection
Fix Diagnostic
Write Tests
Review Diff
```

The agent should receive structured context such as:

- current workspace;
- current file;
- cursor position;
- selection;
- open buffers;
- diagnostics;
- relevant Git diff.

Do not hard-code the entire editor around one AI provider.

Define an agent/backend abstraction once there is a concrete need for it.

## Exit criteria

A human and an agent can productively work in the same TTED workspace while the human remains able to see and understand what the agent is doing.

---

# Phase 10 — Configuration and Personalization

## Goal

Allow customization without requiring configuration just to make TTED pleasant.

TTED must remain excellent with zero configuration.

Add a simple configuration format for things such as:

- tab width;
- indentation style;
- line numbers;
- word wrap;
- theme;
- keybindings;
- explorer behaviour;
- language servers.

Consider TOML unless another format offers a clear advantage.

Avoid building a plugin API yet.

---

# Phase 11 — Polish and Distribution

Prepare TTED to be realistically installable by other people.

## Distribution

Aim for:

```bash
tted .
tted README.md
tted file1.rs file2.rs
```

Eventually provide:

- release binaries;
- Linux x86_64;
- Linux ARM64 where practical;
- macOS where practical;
- installer script;
- package-manager distribution where worthwhile.

## Documentation

Maintain:

- README
- keybindings
- configuration reference
- architecture overview
- agent API reference
- contributing guide
- changelog

## CI

Every change should keep these passing:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Add GitHub CI once appropriate.

---

# v1.0 Definition

TTED 1.0 should mean:

> **I can reasonably choose TTED as my everyday terminal code editor.**

A v1.0-quality TTED should provide:

- reliable conventional text editing;
- excellent keyboard navigation;
- excellent mouse interaction;
- robust Unicode;
- tabs;
- explorer;
- Focus Mode;
- quick file navigation;
- search/replace;
- syntax highlighting;
- Markdown reading;
- Git awareness;
- command palette;
- configuration;
- LSP diagnostics and navigation;
- split editing;
- clean SSH/tmux/Herdr behaviour;
- stable terminal cleanup;
- an agent API;
- an integrated agent area;
- good performance;
- strong tests.

AI must not be required to use any normal editing feature.

---

# Explicit Non-Goals

Unless this roadmap is later changed, TTED should **not** become:

- a tmux replacement;
- a Herdr replacement;
- a shell/session manager;
- a Vim clone;
- an AI-only coding environment;
- a giant IDE framework;
- a web application wrapped in a terminal;
- dependent on a plugin ecosystem for basic usability.

An embedded full terminal is not currently a priority because TTED is expected to run naturally inside tmux, Herdr, SSH, or an ordinary terminal.

Running tools and displaying command output may eventually be useful, but TTED should not duplicate terminal-multiplexer functionality.

---

# Development Rules for the Agent

When implementing this roadmap:

1. **Work phase by phase.**
2. Do not jump to future phases simply because they are interesting.
3. Before each substantial change, inspect the existing implementation rather than assuming how it works.
4. Prefer incremental changes over rewrites.
5. Maintain conventional mouse and keyboard behaviour.
6. Preserve TTED's terminal cleanup guarantees.
7. Preserve Unicode correctness.
8. Add tests for new core behaviour.
9. Keep dependencies intentional and minimal.
10. Run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

before considering a milestone complete.

11. Keep `README.md` and `PROJECT_STATUS.md` synchronized with reality.
12. Maintain this `ROADMAP.md` as implementation progresses.
13. Mark completed items rather than deleting roadmap history.
14. Do not push or publish changes unless explicitly instructed.
15. If completing a feature reveals that a later architectural decision must be made earlier, explain why before introducing a major abstraction.

Most importantly:

> **Prioritize excellent editing UX over feature count.**

A small number of features that feel exceptionally good is more valuable than rapidly checking boxes.

---

# Immediate Next Work

Phase 1 is complete. Begin with **Phase 2 — Workspace and Explorer UX**.

Recommended order:

1. Replace the flattened explorer with a keyboard-navigable directory tree.
2. Add deliberate explorer file operations with confirmations.
3. Improve overflowing tabs and add a close affordance.
4. Implement Focus Mode.
5. Add Quick Open.
6. Expand search and replacement.

Do not start Git, LSP, split panes, or agent integration until the Phase 2 workspace experience is solid.
