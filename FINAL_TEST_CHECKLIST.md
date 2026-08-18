# TTED v0.1 hands-on checklist

Use a normal terminal and, where available, repeat the terminal checks inside
tmux, Herdr, and SSH.

## Launch and terminal

- [ ] `tted .` opens the current workspace with the explorer visible.
- [ ] `tted README.md` opens that file; multiple file arguments create tabs.
- [ ] `tted --help` and `tted --version` print without entering raw mode.
- [ ] Ctrl+Q, an error exit, and a forced terminal resize restore terminal state.

## Editing and files

- [ ] Type Unicode/emoji, select by keyboard/mouse, copy/cut/paste, undo/redo.
- [ ] Enter preserves indentation and brackets indent/dedent as expected.
- [ ] Ctrl+N creates, opens, and focuses a file; Save/Save As refresh explorer.
- [ ] Dirty close and quit confirmations prevent accidental data loss.
- [ ] External file modification/deletion prompts behave safely.

## Navigation and presentation

- [ ] Tabs click/close/cycle; Ctrl+E explorer and Ctrl+P Quick Open work.
- [ ] Ctrl+F find/replace, Ctrl+Shift+P palette, F1 help, F11 Focus Mode work.
- [ ] Syntax colors, line numbers, word wrap, splits, and mouse focus render well.
- [ ] Markdown reader toggles and scrolls by arrows, pages, and mouse wheel.

## Integrations

- [ ] Git branch/state, explorer/gutter marks, status/diffs, and guarded writes work.
- [ ] A configured LSP starts, reports diagnostics, navigates, completes, and stops.
- [ ] Agent API is visible, permissions reject forbidden mutations, stale revisions
      fail, and enabled edits appear in the Agent panel for diff/accept/revert.
- [ ] `.tted.toml` indentation, explorer, theme, wrapping, and custom key settings
      take effect after **Preferences: Reload Configuration**.

## Release gate

- [ ] `cargo fmt --check`
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo build --release`
- [ ] Tagged GitHub release artifacts install successfully on each target platform.
