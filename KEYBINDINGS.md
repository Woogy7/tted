# TTED keybindings

Press F1 in TTED for the built-in quick reference. The most common bindings are:

| Keys | Action |
|---|---|
| Arrows, Home/End, Page Up/Down | Navigate; hold Shift to select |
| Ctrl+N / Ctrl+S / Ctrl+Shift+S | New, Save, Save As |
| Ctrl+A | Select all text in the active document |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, paste |
| Ctrl+Z / Ctrl+Y | Undo, redo |
| Ctrl+F / Ctrl+P | Find/replace, Quick Open |
| F2 / Ctrl+Shift+P | Command Palette (F2 works in every terminal) |
| F3 | View and change keybindings |
| Ctrl+E | Toggle/focus explorer |
| Ctrl+W / Ctrl+Q | Close tab, quit |
| Ctrl+Tab / Ctrl+Shift+Tab | Next/previous tab |
| Alt+Right / Alt+Left | Portable next/previous tab fallback |
| Ctrl+Shift+M or F6 | Markdown source/live preview |
| F8 | Problems / next diagnostic |
| F11 | Focus Mode |
| F1 | Help |

Explorer focus uses arrows, Enter, Left/Right, Home/End, Page Up/Down, mouse
clicks, and wheel scrolling. N creates a file, Shift+N a directory, R renames,
and D requests deletion. Esc or Tab returns to the document.

All advanced actions—including Git, LSP, splits, and configuration
reload—are discoverable in the Command Palette. Custom bindings are documented
in [CONFIGURATION.md](CONFIGURATION.md).

Some terminals send Ctrl+Shift+P as Ctrl+P. In those terminals, use F2 for the
Command Palette instead of Ctrl+Shift+P.

Press F3 to open the Keybindings menu. Select an action with arrows or the
mouse, then press Enter (or click it again) and type the new shortcut. Delete
resets the selected custom binding. Changes are saved for the workspace.

Markdown live preview uses the normal editing keys, including Shift+Home/End,
selection, paste, undo, and find/replace. The active and selected lines show their
source syntax; other lines stay formatted. Mouse clicks position the caret in
text or toggle task checkboxes.
