# TTED configuration

TTED works without configuration. To customize a workspace, copy
`.tted.example.toml` to `.tted.toml` in its root. Set `TTED_CONFIG` to use
another file. Run **Preferences: Reload Configuration** from the Command Palette
after editing it.

## Editor

- `tab_width`: positive indentation/tab-stop width (default `4`).
- `use_spaces`: insert spaces for Tab when true, literal tabs when false.
- `line_numbers`: show line numbers while retaining diagnostic/Git markers.
- `word_wrap`: visually wrap long lines.
- `theme`: bundled syntect syntax theme, such as `base16-eighties.dark`.

TTED's interface uses its built-in Catppuccin Mocha palette; `theme` currently
controls syntax colors.

## Keybindings

The `[keybindings]` table maps normalized key names to command IDs:

```toml
[keybindings]
"ctrl+e" = "workspace.toggle_explorer"
"alt+p" = "workspace.quick_open"
```

Modifiers are `ctrl`, `alt`, and `shift`. Named keys include `enter`,
`esc`, arrows, `home`, `end`, `pageup`, `pagedown`, `tab`,
`backtab`, `delete`, `insert`, `backspace`, and `f1`–`f12`.
Use the Command Palette to discover stable command IDs. Modal dialogs retain
their own input controls.

## Explorer

- `show_hidden`: include dotfiles.
- `show_build_directories`: include `target` and `node_modules`.
- `max_entries`: maximum entries collected by workspace browsing.

## Language servers

Add a table keyed by file extension:

```toml
[language_servers.rs]
command = "rust-analyzer"
args = []
language_id = "rust"
```

The executable must be installed and available in `PATH`.

## Agent API

`[agent]` controls whether the local socket runs and separately grants read,
buffer-write, file-create, file-delete, and command capabilities. Mutating
capabilities default to false. See [AGENT_API.md](AGENT_API.md).
