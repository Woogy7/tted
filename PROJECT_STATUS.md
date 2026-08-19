# TTED project status

TTED v0.1 now implements every phase in the current development roadmap. It is
a standalone conventional terminal editor and remains fully useful without an
agent connection.

## Product surface

- Rope-backed Unicode editing, selection, undo/redo, indentation, clipboard,
  atomic saves, external-change handling, tabs, and unsaved-change prompts.
- Mouse and familiar keyboard navigation, searchable command palette, F1 help,
  Focus Mode, quick open, find/replace, collapsible explorer, and split views.
- File-aware syntax highlighting, configurable line numbers/wrapping, and an
  editable/rendered Markdown toggle.
- Background Git status/diffs and safe stage, unstage, discard, and commit.
- Configured LSP lifecycle, diagnostics, Problems panel, hover, definition,
  completion, references, rename, code actions, formatting, symbols, and
  signature help.
- Permission-scoped local agent JSON-RPC using stable buffer IDs/revisions, plus
  an optional prompt/activity/diff/accept/revert panel.
- F9 built-in Codex chat with automatic CLI detection, device-code sign-in,
  persistent conversation threads, streamed replies/activity/diffs, Stop,
  Retry, New/Clear, role-labeled scrollback, nested-container-safe workspace
  sandboxing, guarded destructive actions, and safe disk-change Accept/Revert.
- Optional workspace TOML for editor, explorer, custom command keys, language
  servers, and agent capabilities. Defaults require no configuration.

## Architecture and operations

Editor state mutates only on the UI loop. Git, LSP, and agent transport use
bounded service/event boundaries; managed children are cancelled and reaped.
Lightweight diagnostic logs default to `/tmp/tted-<pid>.log`.

`tted .`, `tted README.md`, and multiple file arguments are supported. CI
enforces format, tests, Clippy with warnings denied, and release builds. Tagged
releases build checksummed Linux x86_64/ARM64 and macOS x86_64/ARM64 archives.
The installer verifies release checksums, falls back to a Cargo source build,
and configures a normal user's PATH.

## Intentional limitations

- Split layout is deliberately limited to two panes and is not a multiplexer.
- Word wrapping is visual; editing coordinates remain tied to logical lines.
- Undo snapshots are full ropes and syntax highlighting is recomputed for
  visible lines, which are candidates for profiling-led optimization.
- Explorer filtering is configurable but not yet gitignore-aware.
- LSP servers and an agent backend are external optional processes.
- The integrated agent UI is provider-neutral; TTED is not an agent harness.

See `ROADMAP.md` for phase history and `FINAL_TEST_CHECKLIST.md` for the
release-candidate hands-on pass.
