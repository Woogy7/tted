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

1. Press Ctrl+E to open the file explorer.
2. Use Up/Down, Home/End, and Page Up/Down; confirm the highlighted row moves
   and the tree scrolls.
3. Select a folder and use Right/Left to expand and collapse it.
4. Press Enter on a file and confirm a new tab appears. Open it again and confirm
   its existing tab is focused.
5. Repeat folder toggling and file opening with mouse clicks, and scroll with the
   mouse wheel over the explorer.
6. Press N and create a temporary file; use Shift+N to create a temporary folder.
7. Select the temporary file and press R to rename it.
8. Press D on the renamed file. Confirm that N or Esc cancels, then repeat and
   press Y to delete it. Delete the temporary folder the same way.
9. Press Ctrl+E again to return to the document-only layout.

Expected: folders appear before files with clear indentation. Hidden directories
and common build directories are omitted. Esc or Tab returns keyboard focus to
the document.

## 10. New-file dialog and tabs

1. Press Ctrl+N from the document and enter a filename in the centered dialog.
2. Confirm the new file opens immediately and typing goes into its document.
3. Open enough files to overflow the tab bar and cycle tabs with Alt+Left/Right.
4. Confirm the active tab remains visible, then click a tab's `×` to close it.

Expected: new filenames are never entered in the status bar. Dirty tabs still
open a centered dialog and require Y before being discarded.

## 11. Focus Mode

1. Open the explorer, then press F11.
2. Confirm tabs, explorer, and status bar disappear and the document fills the
   terminal.
3. Press F11 again.

Expected: the normal workspace returns with the explorer visible as it was
before entering Focus Mode.

## 12. Quick Open

1. Press Ctrl+P and type a few non-contiguous letters from a workspace path.
2. Use Up/Down and Page Up/Down to move through the matches.
3. Press Enter and confirm the selected file opens, or Esc to cancel.

Expected: matching is case-insensitive, hidden/build folders are absent, and an
already-open result focuses its existing tab.

## 13. Find and Replace

1. Press Ctrl+F, type text that occurs several times, and verify the live count.
2. Use Enter and Shift+Enter for next and previous; toggle case with Alt+C.
3. Press Tab, enter replacement text, and use Ctrl+R for one match.
4. Use Ctrl+Shift+R to replace all remaining matches, then Ctrl+Z once.

Expected: replace-all is undone as one edit and Esc closes the dialog without
changing text.

## 14. Commands and context actions

1. Press Ctrl+Shift+P, fuzzy-search for `focus`, and press Enter.
2. Confirm Focus Mode activates; use the palette again to turn it off.
3. Open the explorer and right-click a file or directory.
4. Confirm the compact file-operation menu appears and Esc closes it.

Expected: palette actions behave identically to their shortcuts, and the
right-click menu exposes New File, New Directory, Rename, and Delete.

## 15. Git awareness

1. Launch TTED in a Git repository and inspect the status bar.
2. Modify, add, and create an untracked file, waiting up to five seconds after
   each change.
3. Open the explorer and inspect the affected file rows.

Expected: the branch is shown with `✓` or `*`, editing never pauses during Git
refreshes, files receive subtle `M`, `A`, `?`, or `D` decorations, and changed
lines receive green added, peach modified, or red deletion gutter markers.

## Deferred features

These are planned but are not expected to work in the current build:

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
- the diagnostic log printed at startup (`/tmp/tted-<pid>.log` by default).
