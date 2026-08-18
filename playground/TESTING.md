# TTED hands-on testing guide

Open the whole sample set as tabs:

```sh
cargo run -- playground/*
```

TTED v0.1 opens files passed on the command line; it does not have the sidebar
file explorer yet.

## 1. Basic navigation

Open `notes.md` and try:

1. Move with all four arrow keys.
2. Use Home and End on short and long lines.
3. Use Page Up and Page Down near the top and bottom of the file.
4. Hold Shift with those keys and check that the selected text is highlighted.
5. Click in several places and drag across text with the mouse.
6. Scroll with the mouse wheel.

Expected: the cursor remains visible and never moves beyond the document. Moving
up and down through short lines should restore the original column on a later
long line.

## 2. Editing and history

Use `example.rb` for destructive experiments:

1. Type text in the middle of a line.
2. Try Backspace and Delete at line boundaries.
3. Select a word and type to replace it.
4. Select several lines and paste a multiline block.
5. Press Ctrl+Z to undo and Ctrl+Y to redo.
6. Insert a newline and enter an indented Ruby method.

Expected: selections are replaced as one edit, pasted text is inserted as one
operation, and undo/redo returns the text and cursor to their earlier positions.

Also check indentation quality of life:

- press Enter after an indented line and confirm its indentation is retained;
- press Enter between `{}` or another bracket pair and check the indented blank line;
- press Tab at different columns and confirm it advances to a four-column stop;
- press Backspace in leading spaces and Shift+Tab to move back one indent level;
- copy with Ctrl+C, move elsewhere, and paste the TTED clipboard with Ctrl+V.

## 3. Tabs

1. Press Ctrl+Tab or Ctrl+PageDown to move to the next tab.
2. Press Ctrl+Shift+Tab or Ctrl+PageUp to move to the previous tab.
3. Make changes in two different tabs.

Expected: each file keeps its own text, cursor, selection, undo history, and dirty
dot. Scroll position is currently reset when changing tabs.

## 4. Saving and quit protection

Use a disposable copy if you want to preserve the original samples:

```sh
cp playground/example.py /tmp/tted-example.py
cargo run -- /tmp/tted-example.py
```

1. Edit the file and look for the dirty dot in its tab.
2. Press Ctrl+S and verify that the dot disappears.
3. Edit again and press Ctrl+Q once.
4. Read the warning in the status bar, then press Ctrl+Q again to discard.

Expected: saving updates the file without corrupting it. A single Ctrl+Q must not
discard unsaved changes.

## 5. Terminal behavior

Run the same tests in an ordinary terminal, tmux, and an SSH session if they are
available.

Check that:

- terminal resizing immediately redraws the editor;
- normal shell input and mouse behavior return after quitting;
- multiline paste is not interpreted as editor shortcuts;
- Ctrl+C does not leave the terminal in raw mode after a process-level exit;
- narrow windows remain usable rather than panicking.

## 6. Unicode and long lines

The Unicode line in `notes.md` contains accented Latin text, Greek, Japanese, and
an emoji. Move through it, select portions, edit it, undo, and save it. In
`example.json`, extend a line beyond the terminal width to exercise horizontal
scrolling.

Expected: wide characters keep the cursor aligned, combined emoji move and delete
as one unit, and moving vertically preserves the visual terminal column.

## 7. Syntax and Markdown reading view

1. Switch among the Rust, Ruby, HTML, CSS, JavaScript, Python, JSON, and Markdown
   samples and confirm that language elements use distinct colors.
2. In a Markdown tab, press Ctrl+Shift+M or F6 to enter the reading view.
3. Scroll with arrows, Page Up/Down, and the mouse wheel.
4. Press the toggle again and confirm that the original editable source returns.

Expected: reading view presents headings, lists, emphasis, quotes, and code without
changing the source. It is intentionally read-only.

## 8. Help, line numbers, and Save As

1. Press F1 and check that the keybindings popup fits the terminal.
2. Close it with Esc or F1.
3. Click beside several line numbers and confirm the cursor lands in the text.
4. Use Ctrl+Shift+S to enter a new path and save a copy.
5. Start TTED without a filename, type something, and press Ctrl+S; the Save As
   prompt should appear automatically.

Expected: the popup blocks edits and mouse actions behind it. Line numbers remain
aligned while scrolling, and a successful Save As updates the tab filename.

## 9. File explorer

1. Press Ctrl+B to open the file explorer.
2. Click a file that is not open and confirm a new tab appears.
3. Click a file that is already open and confirm its existing tab is focused.
4. Press Ctrl+B again to return to the document-only layout.

Expected: hidden directories and `target` are omitted, while files below ordinary
workspace folders are shown by relative path.

## Deferred features

These are planned but are not expected to work in the current build:

- syntax coloring;
- file explorer and Save As;
- Markdown preview;
- split panes and configurable key bindings;
- Git status, LSP, and agent integration.

## Useful bug report details

When something feels wrong, capture:

- the file and exact line involved;
- the keys or mouse actions used;
- expected and actual behavior;
- terminal name and version;
- whether it also happens outside tmux/SSH;
- a minimal text sample, especially for Unicode issues.
