# TTED agent API

TTED exposes newline-delimited JSON-RPC 2.0 on a local Unix-domain socket. The
default socket is `/tmp/tted-<pid>.sock`; set `TTED_SOCKET` to override it. The
socket is created with mode `0600` and removed during graceful shutdown.

Most users do not need this API. Press F9 for TTED's automatically managed
built-in Codex chat; see [AGENT_CHAT.md](AGENT_CHAT.md). This API remains the
provider-neutral integration point for custom external agents.

Example request:

```json
{"jsonrpc":"2.0","id":1,"method":"editor.current_file","params":{}}
```

Each request receives one JSON response line. Buffer IDs are stable for the
buffer lifetime. Every mutation requires the revision returned by `buffer.read`
or `buffer.revision`; stale revisions are rejected before edits are applied.

## Methods

Read capability:

- `workspace.info`, `workspace.files`
- `buffer.list`, `buffer.read`, `buffer.revision`
- `editor.current_file`, `editor.cursor`, `editor.selection`
- `diagnostics.list`, `git.status`, `git.diff`

Write capability:

- `editor.open { path }`
- `editor.focus_range { buffer_id, line, column }`
- `edit.apply { buffer_id, revision, start, end, text }`
- `edit.apply_batch { edits: [...] }`

Optional capabilities:

- `file.create { path }`
- `file.delete { path }` (only closed files; non-recursive directories)
- `command.run { id }` invokes the same stable command used by the UI

Integrated panel bridge:

- `agent.next_prompt` retrieves the next human prompt, or `null`
- `agent.respond { text, append }` streams response text into the panel
- `agent.activity { text, path? }` reports visible activity; paths are clickable

Paths are workspace-relative and cannot escape the workspace. Configure access
under `[agent]` in `.tted.toml`. Read access defaults on; all mutation, file,
and command capabilities default off. TTED remains fully usable with the API
disabled.
