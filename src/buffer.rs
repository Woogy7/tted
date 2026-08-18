use std::{
    fs, io,
    path::{Path, PathBuf},
};

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Snapshot {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
}

pub struct Buffer {
    text: Rope,
    path: Option<PathBuf>,
    cursor: usize,
    anchor: Option<usize>,
    preferred_col: Option<usize>,
    revision: u64,
    saved_revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    crlf: bool,
}

impl Buffer {
    pub fn empty() -> Self {
        Self::from_text(String::new(), None, false)
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;
        let source = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "TTED v0.1 only edits UTF-8 text",
            )
        })?;
        let crlf = source.contains("\r\n");
        let normalized = if crlf {
            source.replace("\r\n", "\n")
        } else {
            source
        };
        Ok(Self::from_text(normalized, Some(path.to_path_buf()), crlf))
    }

    fn from_text(text: String, path: Option<PathBuf>, crlf: bool) -> Self {
        Self {
            text: Rope::from_str(&text),
            path,
            cursor: 0,
            anchor: None,
            preferred_col: None,
            revision: 0,
            saved_revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            crlf,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
            .to_owned()
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }
    pub fn text(&self) -> String {
        self.text.to_string()
    }
    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }
    pub fn line(&self, line: usize) -> String {
        self.text.line(line).to_string()
    }
    pub fn line_start_char(&self, line: usize) -> usize {
        self.text
            .line_to_char(line.min(self.text.len_lines().saturating_sub(1)))
    }
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let line = self.text.char_to_line(self.cursor);
        (line, self.cursor - self.text.line_to_char(line))
    }
    pub fn cursor_screen_col(&self) -> usize {
        let (line, _) = self.cursor_line_col();
        let start = self.text.line_to_char(line);
        UnicodeWidthStr::width(self.text.slice(start..self.cursor).to_string().as_str())
    }
    pub fn current_line_prefix(&self) -> String {
        let (line, _) = self.cursor_line_col();
        let start = self.text.line_to_char(line);
        self.text.slice(start..self.cursor).to_string()
    }
    pub fn char_at_cursor(&self) -> Option<char> {
        (self.cursor < self.text.len_chars()).then(|| self.text.char(self.cursor))
    }
    pub fn find_next(&mut self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        let text = self.text.to_string();
        let cursor_byte = self.text.char_to_byte(self.cursor);
        let found_byte = text[cursor_byte..]
            .find(query)
            .map(|offset| cursor_byte + offset)
            .or_else(|| text[..cursor_byte].find(query));
        let Some(start_byte) = found_byte else {
            return false;
        };
        let end_byte = start_byte + query.len();
        let start = self.text.byte_to_char(start_byte);
        let end = self.text.byte_to_char(end_byte);
        self.anchor = Some(start);
        self.cursor = end;
        self.preferred_col = None;
        true
    }
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.filter(|a| *a != self.cursor).map(|a| {
            if a < self.cursor {
                (a, self.cursor)
            } else {
                (self.cursor, a)
            }
        })
    }
    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|(start, end)| self.text.slice(start..end).to_string())
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.checkpoint();
        self.delete_selection_raw();
        self.finish_edit();
        Some(text)
    }

    fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.redo.clear();
    }
    fn finish_edit(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.preferred_col = None;
    }
    fn delete_selection_raw(&mut self) -> bool {
        if let Some((start, end)) = self.selection() {
            self.text.remove(start..end);
            self.cursor = start;
            self.anchor = None;
            true
        } else {
            false
        }
    }
    pub fn insert(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        self.checkpoint();
        self.delete_selection_raw();
        self.text.insert(self.cursor, value);
        self.cursor += value.chars().count();
        self.anchor = None;
        self.finish_edit();
    }
    pub fn backspace(&mut self) {
        if self.selection().is_none() && self.cursor == 0 {
            return;
        }
        self.checkpoint();
        if !self.delete_selection_raw() {
            let previous = self.previous_grapheme_boundary(self.cursor);
            self.text.remove(previous..self.cursor);
            self.cursor = previous;
        }
        self.anchor = None;
        self.finish_edit();
    }
    pub fn smart_backspace(&mut self, tab_width: usize) {
        if self.selection().is_some() {
            self.backspace();
            return;
        }
        let prefix = self.current_line_prefix();
        if !prefix.is_empty() && prefix.chars().all(|ch| ch == ' ') {
            let col = prefix.chars().count();
            let remainder = col % tab_width;
            let count = if remainder == 0 {
                col.min(tab_width)
            } else {
                remainder
            };
            self.checkpoint();
            self.text.remove(self.cursor - count..self.cursor);
            self.cursor -= count;
            self.anchor = None;
            self.finish_edit();
        } else {
            self.backspace();
        }
    }

    pub fn unindent_current_line(&mut self, tab_width: usize) {
        let (line, _) = self.cursor_line_col();
        let start = self.text.line_to_char(line);
        let available = self.text.len_chars().saturating_sub(start).min(tab_width);
        let count = (0..available)
            .take_while(|offset| self.text.char(start + offset) == ' ')
            .count();
        if count == 0 {
            return;
        }
        self.checkpoint();
        self.text.remove(start..start + count);
        self.cursor = self.cursor.saturating_sub(count);
        self.anchor = None;
        self.finish_edit();
    }
    pub fn delete_forward(&mut self) {
        if self.selection().is_none() && self.cursor == self.text.len_chars() {
            return;
        }
        self.checkpoint();
        if !self.delete_selection_raw() {
            let next = self.next_grapheme_boundary(self.cursor);
            self.text.remove(self.cursor..next);
        }
        self.anchor = None;
        self.finish_edit();
    }
    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(Snapshot {
                text: self.text.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.text = previous.text;
            self.cursor = previous.cursor;
            self.anchor = previous.anchor;
            self.revision = self.revision.wrapping_add(1);
            self.preferred_col = None;
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(Snapshot {
                text: self.text.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.text = next.text;
            self.cursor = next.cursor;
            self.anchor = next.anchor;
            self.revision = self.revision.wrapping_add(1);
            self.preferred_col = None;
        }
    }
    fn begin_move(&mut self, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }
    pub fn move_horizontal(&mut self, delta: isize, select: bool) {
        self.begin_move(select);
        for _ in 0..delta.unsigned_abs() {
            self.cursor = if delta < 0 {
                self.previous_grapheme_boundary(self.cursor)
            } else {
                self.next_grapheme_boundary(self.cursor)
            };
        }
        self.preferred_col = None;
    }
    pub fn move_vertical(&mut self, delta: isize, select: bool) {
        self.begin_move(select);
        let (line, _) = self.cursor_line_col();
        let screen_col = self.cursor_screen_col();
        let wanted = *self.preferred_col.get_or_insert(screen_col);
        let target = line
            .saturating_add_signed(delta)
            .min(self.text.len_lines().saturating_sub(1));
        self.cursor = self.char_at_screen_col(target, wanted);
    }
    pub fn move_line_edge(&mut self, end: bool, select: bool) {
        self.begin_move(select);
        let (line, _) = self.cursor_line_col();
        let start = self.text.line_to_char(line);
        let next = if line + 1 < self.text.len_lines() {
            self.text.line_to_char(line + 1)
        } else {
            self.text.len_chars()
        };
        self.cursor = if end && next > start && self.text.char(next - 1) == '\n' {
            next - 1
        } else if end {
            next
        } else {
            start
        };
        self.preferred_col = None;
    }
    pub fn set_cursor_line_col(&mut self, line: usize, col: usize, select: bool) {
        self.begin_move(select);
        let line = line.min(self.text.len_lines().saturating_sub(1));
        let start = self.text.line_to_char(line);
        let raw = self.text.line(line);
        let max = raw.len_chars()
            - usize::from(raw.len_chars() > 0 && raw.char(raw.len_chars() - 1) == '\n');
        self.cursor = start + col.min(max);
        self.preferred_col = None;
    }

    pub fn set_cursor_line_screen_col(&mut self, line: usize, screen_col: usize, select: bool) {
        self.begin_move(select);
        let line = line.min(self.text.len_lines().saturating_sub(1));
        self.cursor = self.char_at_screen_col(line, screen_col);
        self.preferred_col = None;
    }

    fn previous_grapheme_boundary(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        let text = self.text.to_string();
        let byte = self.text.char_to_byte(index);
        text[..byte]
            .grapheme_indices(true)
            .next_back()
            .map(|(boundary, _)| self.text.byte_to_char(boundary))
            .unwrap_or(0)
    }

    fn next_grapheme_boundary(&self, index: usize) -> usize {
        if index >= self.text.len_chars() {
            return self.text.len_chars();
        }
        let text = self.text.to_string();
        let byte = self.text.char_to_byte(index);
        let length = text[byte..]
            .graphemes(true)
            .next()
            .map(str::len)
            .unwrap_or(0);
        self.text.byte_to_char(byte + length)
    }

    fn char_at_screen_col(&self, line: usize, wanted: usize) -> usize {
        let start = self.text.line_to_char(line);
        let mut content = self.text.line(line).to_string();
        while content.ends_with(['\n', '\r']) {
            content.pop();
        }
        let mut screen = 0usize;
        let mut chars = 0usize;
        for grapheme in content.graphemes(true) {
            let next = screen + UnicodeWidthStr::width(grapheme);
            if next > wanted {
                break;
            }
            screen = next;
            chars += grapheme.chars().count();
        }
        start + chars
    }
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "no path; use a filename argument for v0.1",
                )
            })?
            .clone();
        self.write_to(&path)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref().to_path_buf();
        self.write_to(&path)?;
        self.path = Some(path);
        Ok(())
    }

    fn write_to(&mut self, path: &Path) -> io::Result<()> {
        let mut text = self.text.to_string();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        let temp = path.with_extension(format!(
            "{}.tted-tmp",
            path.extension().and_then(|x| x.to_str()).unwrap_or("")
        ));
        fs::write(&temp, text.as_bytes())?;
        fs::rename(temp, path)?;
        self.saved_revision = self.revision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_and_undo() {
        let mut b = Buffer::empty();
        b.insert("hello");
        b.backspace();
        assert_eq!(b.text.to_string(), "hell");
        b.undo();
        assert_eq!(b.text.to_string(), "hello");
        b.redo();
        assert_eq!(b.text.to_string(), "hell");
    }
    #[test]
    fn movement_keeps_column() {
        let mut b = Buffer::from_text("abcd\nx\nabcd".into(), None, false);
        b.set_cursor_line_col(0, 3, false);
        b.move_vertical(1, false);
        assert_eq!(b.cursor_line_col(), (1, 1));
        b.move_vertical(1, false);
        assert_eq!(b.cursor_line_col(), (2, 3));
    }
    #[test]
    fn selection_is_replaced() {
        let mut b = Buffer::from_text("abcd".into(), None, false);
        b.move_horizontal(2, true);
        b.insert("X");
        assert_eq!(b.text.to_string(), "Xcd");
    }
    #[test]
    fn cut_is_one_undoable_edit() {
        let mut b = Buffer::from_text("hello".into(), None, false);
        b.move_horizontal(2, true);
        assert_eq!(b.cut_selection().as_deref(), Some("he"));
        assert_eq!(b.text.to_string(), "llo");
        b.undo();
        assert_eq!(b.text.to_string(), "hello");
    }
    #[test]
    fn smart_backspace_uses_tab_stops() {
        let mut b = Buffer::from_text("      value".into(), None, false);
        b.set_cursor_line_col(0, 6, false);
        b.smart_backspace(4);
        assert_eq!(b.text.to_string(), "    value");
        b.smart_backspace(4);
        assert_eq!(b.text.to_string(), "value");
    }
    #[test]
    fn find_wraps_and_selects_match() {
        let mut b = Buffer::from_text("one two one".into(), None, false);
        b.set_cursor_line_col(0, 5, false);
        assert!(b.find_next("one"));
        assert_eq!(b.selection(), Some((8, 11)));
        assert!(b.find_next("one"));
        assert_eq!(b.selection(), Some((0, 3)));
    }
    #[test]
    fn save_as_assigns_path_and_writes_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new-file.txt");
        let mut b = Buffer::empty();
        b.insert("saved text");
        b.save_as(&path).unwrap();
        assert_eq!(b.path(), Some(path.as_path()));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "saved text");
        assert!(!b.is_dirty());
    }
    #[test]
    fn movement_and_deletion_respect_graphemes() {
        let mut b = Buffer::from_text("a👩‍💻b".into(), None, false);
        b.move_horizontal(1, false);
        assert_eq!(b.cursor(), 1);
        b.move_horizontal(1, false);
        assert_eq!(b.cursor(), 4);
        b.backspace();
        assert_eq!(b.text.to_string(), "ab");
        assert_eq!(b.cursor(), 1);
    }

    #[test]
    fn vertical_movement_preserves_screen_column() {
        let mut b = Buffer::from_text("日本語\nabcdef".into(), None, false);
        b.move_horizontal(2, false);
        assert_eq!(b.cursor_screen_col(), 4);
        b.move_vertical(1, false);
        assert_eq!(b.cursor_line_col(), (1, 4));
    }
}
