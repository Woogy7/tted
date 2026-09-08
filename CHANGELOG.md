# Changelog

All notable changes are documented here.

## Unreleased

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
