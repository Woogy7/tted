# TTED project status

TTED is a standalone conventional terminal editor. Current development focuses
on safe editing, responsive navigation, and working alongside external tools.
Historical agent phases in the roadmap are retired.

## Product surface

- Rope-backed Unicode editing, selection, undo/redo, indentation, clipboard,
  atomic saves, external-change handling, tabs, and unsaved-change prompts.
- Mouse and familiar keyboard navigation, searchable command palette, F1 help,
  Focus Mode, quick open, find/replace, collapsible explorer, and split views.
- File-aware syntax highlighting, configurable line numbers/wrapping, and an
  editable Markdown live preview with a full-source toggle.
- Background Git status/diffs and safe stage, unstage, discard, and commit.
- Configured LSP lifecycle, diagnostics, Problems panel, hover, definition,
  completion, references, rename, code actions, formatting, symbols, and
  signature help.
- Optional workspace TOML for editor, explorer, custom command keys, and language
  servers. Defaults require no configuration.

## Architecture and operations

Editor state mutates only on the UI loop. Git and LSP communicate through
service events; managed language-server children are cancelled and reaped.
Lightweight diagnostic logs default to `/tmp/tted-<pid>.log`.

`tted .`, `tted README.md`, and multiple file arguments are supported. CI
enforces format, tests, Clippy with warnings denied, and release builds. Tagged
releases build checksummed Linux x86_64/ARM64 and macOS x86_64/ARM64 archives.
The installer verifies release checksums, falls back to a Cargo source build,
and configures a normal user's PATH.

## Intentional limitations

- Split layout is deliberately limited to two panes and is not a multiplexer.
- Word wrapping is visual; editing coordinates remain tied to logical lines.
- Undo snapshots share rope storage. Syntax highlighting caches parser states
  and visible styles, yielding during long scans; lines over 16 KiB omit colors.
- Search results are cached by revision; case-insensitive matching handles Unicode
  lowercase expansions.
- Explorer filtering is configurable but not yet gitignore-aware.
- Language servers are optional external processes, with one active language
  service at a time. Completion and code actions still expose a basic protocol
  subset; they are candidates for further usability work.
- External tools edit files directly; there is no built-in chat or agent API.

See `ROADMAP.md` for phase history and `FINAL_TEST_CHECKLIST.md` for the
release-candidate hands-on pass.

## September 2026 editing improvements

Integrated agent features have been removed. Saves preserve file permissions and
symlink targets, confirm Save As overwrites, and check for disk conflicts before
replacement. External reloads preserve earlier undo history and cursor/selection
context; even discarded unsaved edits can be recovered with Undo.

## Validation — 2026-09-08

115 automated tests, formatting, Clippy with warnings denied, and the release
build pass. Real pseudo-terminal checks cover paste/save, permissions, external
reload and undo, dirty conflicts, resize, and terminal restoration. A controlled
language-server process verifies document open/close, stale-format rejection,
and child cleanup. Git refresh tests use temporary repositories. Cross-platform
release installation and interactive SSH/tmux testing remain release checks.

Markdown live preview supports normal editing and selection, revealing syntax on
the active/selected lines while retaining live styling. Parsed Markdown is cached
by revision. Shift+Home/End uses the same selection behavior as source mode.
Diagnostics now aggregate frame-render and input-processing timings without
recording document contents. Unchanged Git snapshots no longer redraw the UI,
and quick Git commands use a shorter polling interval.

Markdown editing now continues lists/tasks/quotes, supports whole-line list
indentation and backtick completion, and shares its parsed styles across source
and preview. Both `te` and `tted` launch the same editor. Terminal and offline
installer checks cover both launchers and older release archives. Physical
laptop shortcut forwarding can be inspected through Help: Inspect Next Key.
