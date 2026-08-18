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
- `agent_backend.rs`: managed provider-neutral chat backend; the first adapter
  speaks the supported Codex app-server stdio protocol.
- `config.rs`: zero-config defaults and TOML loading.

A file is persisted data, a buffer owns editable text, a tab selects an open
buffer, and a split pane is a view referencing a buffer. Special content reuses
buffers, panels, and popups instead of introducing a second UI framework.

Rope revisions guard external/agent edits against stale state. Terminal setup
is protected by an RAII guard so raw mode, alternate screen, mouse reporting,
focus reporting, bracketed paste, and cursor visibility are restored on exit.

The built-in Codex backend normally uses App Server with workspace-write roots
limited to the active workspace. Direct network namespace isolation is avoided
because Bubblewrap cannot configure loopback in some nested containers. When
`CODEX_PERMISSION_PROFILE` proves TTED already runs inside a Codex-enforced
filesystem boundary, App Server receives `externalSandbox` rather than trying
to nest Bubblewrap; the parent boundary remains authoritative. TTED never uses
that fallback merely because Bubblewrap is missing or fails.
