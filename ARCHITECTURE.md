# TTED architecture

TTED is one Rust crate with a single-threaded editor state model. Terminal
events, commands, and completed service events converge on the
editor loop; background workers never mutate UI state directly.

- `file_io.rs`: atomic file replacement, metadata checks, and save destinations.
- `buffer.rs`: Ropey text, cursors, selections, revisions, undo, persistence.
- `syntax.rs`: incremental parser checkpoints and visible-line highlighting cache.
- `editor.rs`: files/tabs/views, commands, input routing, layout, rendering.
- `explorer.rs`: workspace tree and navigation.
- `command.rs`: shared stable command registry and palette.
- `service.rs`: small cancellable worker and managed-process primitives.
- `language_edits.rs`: validate complete LSP edit batches against buffer revisions.
- `git.rs`, `lsp.rs`: typed background integrations.
- `config.rs`: zero-config defaults and TOML loading.

A file is persisted data, a buffer owns editable text, a tab selects an open
buffer, and a split pane is a view referencing a buffer. Special content reuses
buffers, panels, and popups instead of introducing a second UI framework.

Rope revisions guard programmatic range edits against stale state. Terminal setup
is protected by an RAII guard so raw mode, alternate screen, mouse reporting,
focus reporting, bracketed paste, and cursor visibility are restored on exit.
