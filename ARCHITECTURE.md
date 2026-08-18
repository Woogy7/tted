# TTED architecture

TTED is one Rust crate with a single-threaded editor state model. Terminal
events, commands, agent requests, and completed service events converge on the
editor loop; background workers never mutate UI state directly.

- `buffer.rs`: Ropey text, cursors, selections, revisions, undo, persistence.
- `editor.rs`: files/tabs/views, commands, input routing, layout, rendering.
- `explorer.rs`: workspace tree and navigation.
- `command.rs`: shared stable command registry and palette.
- `service.rs`: small cancellable worker and managed-process primitives.
- `git.rs`, `lsp.rs`: typed background integrations.
- `agent.rs`: permission-scoped local JSON-RPC transport.
- `config.rs`: zero-config defaults and TOML loading.

A file is persisted data, a buffer owns editable text, a tab selects an open
buffer, and a split pane is a view referencing a buffer. Special content reuses
buffers, panels, and popups instead of introducing a second UI framework.

Rope revisions guard external/agent edits against stale state. Terminal setup
is protected by an RAII guard so raw mode, alternate screen, mouse reporting,
focus reporting, bracketed paste, and cursor visibility are restored on exit.
