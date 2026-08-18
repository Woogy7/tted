use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Snapshot {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
    content_id: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditGroup {
    Typing,
    Backspace,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalChange {
    None,
    Modified,
    Deleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

pub struct Buffer {
    text: Rope,
    path: Option<PathBuf>,
    cursor: usize,
    anchor: Option<usize>,
    preferred_col: Option<usize>,
    revision: u64,
    content_id: u64,
    saved_content_id: u64,
    next_content_id: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    edit_group: Option<(EditGroup, Instant)>,
    crlf: bool,
    disk_stamp: Option<FileStamp>,
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
        let disk_stamp = path.as_deref().and_then(file_stamp);
        Self {
            text: Rope::from_str(&text),
            path,
            cursor: 0,
            anchor: None,
            preferred_col: None,
            revision: 0,
            content_id: 0,
            saved_content_id: 0,
            next_content_id: 1,
            undo: Vec::new(),
            redo: Vec::new(),
            edit_group: None,
            crlf,
            disk_stamp,
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
        self.content_id != self.saved_content_id
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
        self.push_undo_snapshot();
        self.redo.clear();
        self.edit_group = None;
    }
    fn checkpoint_grouped(&mut self, group: EditGroup) {
        let now = Instant::now();
        let continues = self.edit_group.is_some_and(|(active, at)| {
            active == group && now.duration_since(at) < Duration::from_secs(1)
        });
        if !continues {
            self.push_undo_snapshot();
            self.redo.clear();
        }
        self.edit_group = Some((group, now));
    }
    fn push_undo_snapshot(&mut self) {
        self.undo.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
            content_id: self.content_id,
        });
    }
    fn finish_edit(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.content_id = self.next_content_id;
        self.next_content_id = self.next_content_id.wrapping_add(1);
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
    pub fn insert_typed(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        if self.selection().is_some() {
            self.checkpoint();
        } else {
            self.checkpoint_grouped(EditGroup::Typing);
        }
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
        if self.selection().is_some() {
            self.checkpoint();
        } else {
            self.checkpoint_grouped(EditGroup::Backspace);
        }
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
        if self.selection().is_some() {
            self.checkpoint();
        } else {
            self.checkpoint_grouped(EditGroup::Delete);
        }
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
                content_id: self.content_id,
            });
            self.text = previous.text;
            self.cursor = previous.cursor;
            self.anchor = previous.anchor;
            self.content_id = previous.content_id;
            self.revision = self.revision.wrapping_add(1);
            self.preferred_col = None;
            self.edit_group = None;
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(Snapshot {
                text: self.text.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
                content_id: self.content_id,
            });
            self.text = next.text;
            self.cursor = next.cursor;
            self.anchor = next.anchor;
            self.content_id = next.content_id;
            self.revision = self.revision.wrapping_add(1);
            self.preferred_col = None;
            self.edit_group = None;
        }
    }
    fn begin_move(&mut self, select: bool) {
        self.edit_group = None;
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
        let line = self.text.char_to_line(index);
        let line_start = self.text.line_to_char(line);
        if index == line_start {
            return index - 1;
        }
        let prefix = self.text.slice(line_start..index).to_string();
        prefix
            .grapheme_indices(true)
            .next_back()
            .map(|(boundary, _)| line_start + prefix[..boundary].chars().count())
            .unwrap_or(line_start)
    }

    fn next_grapheme_boundary(&self, index: usize) -> usize {
        if index >= self.text.len_chars() {
            return self.text.len_chars();
        }
        let line = self.text.char_to_line(index);
        let line_end = if line + 1 < self.text.len_lines() {
            self.text.line_to_char(line + 1)
        } else {
            self.text.len_chars()
        };
        let suffix = self.text.slice(index..line_end).to_string();
        let length = suffix
            .graphemes(true)
            .next()
            .map(|grapheme| grapheme.chars().count())
            .unwrap_or(0);
        index + length
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
        self.saved_content_id = self.content_id;
        self.edit_group = None;
        self.disk_stamp = file_stamp(path);
        Ok(())
    }

    pub fn check_external_change(&self) -> io::Result<ExternalChange> {
        let Some(path) = self.path() else {
            return Ok(ExternalChange::None);
        };
        match fs::metadata(path) {
            Ok(metadata) => {
                let current = stamp_from_metadata(&metadata);
                Ok(match self.disk_stamp {
                    Some(known) if known == current => ExternalChange::None,
                    Some(_) | None => ExternalChange::Modified,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(if self.disk_stamp.is_some() {
                    ExternalChange::Deleted
                } else {
                    ExternalChange::None
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot reload an untitled buffer",
            )
        })?;
        let bytes = fs::read(path)?;
        let source = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "external file is not valid UTF-8",
            )
        })?;
        self.crlf = source.contains("\r\n");
        let normalized = if self.crlf {
            source.replace("\r\n", "\n")
        } else {
            source
        };
        self.text = Rope::from_str(&normalized);
        self.cursor = self.cursor.min(self.text.len_chars());
        self.anchor = None;
        self.preferred_col = None;
        self.undo.clear();
        self.redo.clear();
        self.edit_group = None;
        self.revision = self.revision.wrapping_add(1);
        self.content_id = self.next_content_id;
        self.next_content_id = self.next_content_id.wrapping_add(1);
        self.saved_content_id = self.content_id;
        self.disk_stamp = file_stamp(path);
        Ok(())
    }

    pub fn keep_after_external_change(&mut self) {
        self.disk_stamp = self.path.as_deref().and_then(file_stamp);
        if !self.is_dirty() {
            self.content_id = self.next_content_id;
            self.next_content_id = self.next_content_id.wrapping_add(1);
        }
        self.edit_group = None;
    }
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    fs::metadata(path)
        .ok()
        .map(|metadata| stamp_from_metadata(&metadata))
}

fn stamp_from_metadata(metadata: &fs::Metadata) -> FileStamp {
    FileStamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
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

    #[test]
    fn sequential_typing_undoes_as_one_transaction() {
        let mut b = Buffer::empty();
        for character in "hello world".chars() {
            b.insert_typed(&character.to_string());
        }
        b.undo();
        assert_eq!(b.text(), "");
        b.redo();
        assert_eq!(b.text(), "hello world");
    }

    #[test]
    fn movement_breaks_typing_transaction() {
        let mut b = Buffer::empty();
        b.insert_typed("a");
        b.insert_typed("b");
        b.move_horizontal(-1, false);
        b.insert_typed("X");
        b.undo();
        assert_eq!(b.text(), "ab");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn undoing_to_saved_content_clears_dirty_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dirty.txt");
        let mut b = Buffer::empty();
        b.insert("saved");
        b.save_as(path).unwrap();
        b.insert_typed("!");
        assert!(b.is_dirty());
        b.undo();
        assert!(!b.is_dirty());
        b.redo();
        assert!(b.is_dirty());
    }

    #[test]
    fn detects_and_reloads_external_change_when_clean() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("external.txt");
        fs::write(&path, "before").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        fs::write(&path, "after, with a different size").unwrap();
        assert_eq!(b.check_external_change().unwrap(), ExternalChange::Modified);
        b.reload_from_disk().unwrap();
        assert_eq!(b.text(), "after, with a different size");
        assert!(!b.is_dirty());
        assert_eq!(b.check_external_change().unwrap(), ExternalChange::None);
    }

    #[test]
    fn keeping_external_change_preserves_editor_text_and_marks_dirty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("external.txt");
        fs::write(&path, "editor version").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        fs::write(&path, "disk version with another size").unwrap();
        b.keep_after_external_change();
        assert_eq!(b.text(), "editor version");
        assert!(b.is_dirty());
        assert_eq!(b.check_external_change().unwrap(), ExternalChange::None);
    }

    #[test]
    fn detects_deleted_file_and_keep_allows_recreation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("deleted.txt");
        fs::write(&path, "keep me").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(b.check_external_change().unwrap(), ExternalChange::Deleted);
        b.keep_after_external_change();
        assert!(b.is_dirty());
        b.save().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "keep me");
    }

    #[test]
    fn detects_file_recreated_after_deleted_version_was_kept() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recreated.txt");
        fs::write(&path, "original").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        fs::remove_file(&path).unwrap();
        b.keep_after_external_change();
        fs::write(&path, "recreated with a different size").unwrap();
        assert_eq!(b.check_external_change().unwrap(), ExternalChange::Modified);
    }

    #[test]
    fn crlf_and_missing_final_newline_survive_edit_and_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("windows.txt");
        fs::write(&path, b"one\r\ntwo").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        b.set_cursor_line_col(1, 3, false);
        b.insert("!");
        b.save().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"one\r\ntwo!");
    }

    #[test]
    fn multiline_selection_replacement_is_atomic() {
        let mut b = Buffer::from_text("one\ntwo\nthree".into(), None, false);
        b.set_cursor_line_col(0, 1, false);
        b.set_cursor_line_col(2, 2, true);
        b.insert("X");
        assert_eq!(b.text(), "oXree");
        b.undo();
        assert_eq!(b.text(), "one\ntwo\nthree");
    }

    #[test]
    fn multiline_paste_is_one_undo_transaction() {
        let mut b = Buffer::empty();
        b.insert("first\nsecond\nthird");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn long_line_editing_stays_correct() {
        let text = "a".repeat(200_000);
        let mut b = Buffer::from_text(text, None, false);
        b.set_cursor_line_col(0, 199_999, false);
        b.move_horizontal(1, false);
        b.insert_typed("界");
        assert_eq!(b.len_chars(), 200_001);
        b.undo();
        assert_eq!(b.len_chars(), 200_000);
    }

    #[test]
    fn large_buffer_editing_sanity() {
        let text = "0123456789abcdef".repeat(5) + "\n";
        let text = text.repeat(20_000);
        let started = Instant::now();
        let mut b = Buffer::from_text(text, None, false);
        b.set_cursor_line_col(19_999, 40, false);
        b.insert_typed("x");
        b.move_horizontal(-1, false);
        b.delete_forward();
        b.undo();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(b.len_lines(), 20_001);
    }
}
