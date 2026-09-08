use std::{
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

use crate::file_io::{absolute_path, atomic_write, file_stamp, read_document, FileStamp};
use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Snapshot {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
    content_id: u64,
    crlf: bool,
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

struct SearchCache {
    revision: u64,
    query: String,
    case_sensitive: bool,
    matches: Rc<Vec<(usize, usize)>>,
}

pub struct Buffer {
    id: u64,
    text: Rope,
    path: Option<PathBuf>,
    virtual_name: Option<String>,
    read_only: bool,
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
    search_cache: RefCell<Option<SearchCache>>,
}

impl Buffer {
    pub fn empty() -> Self {
        Self::from_text(String::new(), None, false)
    }

    pub fn read_only(name: impl Into<String>, text: String) -> Self {
        let mut buffer = Self::from_text(text, None, false);
        buffer.virtual_name = Some(name.into());
        buffer.read_only = true;
        buffer
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = absolute_path(path.as_ref())?;
        let (text, crlf, stamp) = read_document(&path)?;
        let mut buffer = Self::from_text(text, Some(path), crlf);
        buffer.disk_stamp = stamp;
        Ok(buffer)
    }

    fn from_text(text: String, path: Option<PathBuf>, crlf: bool) -> Self {
        let disk_stamp = path
            .as_deref()
            .and_then(|path| file_stamp(path).ok().flatten());
        Self {
            id: NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed),
            text: Rope::from_str(&text),
            path,
            virtual_name: None,
            read_only: false,
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
            search_cache: RefCell::new(None),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn name(&self) -> String {
        if let Some(name) = &self.virtual_name {
            return name.clone();
        }
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
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn replace_range(
        &mut self,
        expected_revision: u64,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<u64, &'static str> {
        self.replace_ranges(expected_revision, &[(start, end, text.to_owned())])
    }

    pub fn replace_ranges(
        &mut self,
        expected_revision: u64,
        edits: &[(usize, usize, String)],
    ) -> Result<u64, &'static str> {
        if self.read_only {
            return Err("buffer is read-only");
        }
        if self.revision != expected_revision {
            return Err("stale buffer revision");
        }
        if edits
            .iter()
            .any(|(start, end, _)| start > end || *end > self.text.len_chars())
        {
            return Err("invalid edit range");
        }
        let mut edits = edits.to_vec();
        edits.sort_unstable_by_key(|edit| std::cmp::Reverse(edit.0));
        if edits
            .windows(2)
            .any(|pair| pair[1].1 > pair[0].0 || pair[1].0 == pair[0].0)
        {
            return Err("overlapping edit ranges");
        }
        if edits.is_empty() {
            return Ok(self.revision);
        }
        self.checkpoint();
        for (start, end, text) in edits {
            self.text.remove(start..end);
            self.text.insert(start, &text);
            self.cursor = start + text.chars().count();
        }
        self.anchor = None;
        self.finish_edit();
        Ok(self.revision)
    }
    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }
    pub(crate) fn rope(&self) -> &Rope {
        &self.text
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
        self.find_search(query, true, false).is_some()
    }

    pub fn find_search(
        &mut self,
        query: &str,
        case_sensitive: bool,
        backwards: bool,
    ) -> Option<(usize, usize)> {
        let matches = self.search_ranges(query, case_sensitive);
        if matches.is_empty() {
            return None;
        }
        let current_start = self.selection().map_or(self.cursor, |(start, _)| start);
        let index = if backwards {
            matches
                .iter()
                .rposition(|(_, end)| *end <= current_start)
                .unwrap_or(matches.len() - 1)
        } else {
            matches
                .iter()
                .position(|(start, _)| *start >= self.cursor)
                .unwrap_or(0)
        };
        let (start, end) = matches[index];
        self.anchor = Some(start);
        self.cursor = end;
        self.preferred_col = None;
        Some((index + 1, matches.len()))
    }

    pub fn refresh_search(&mut self, query: &str, case_sensitive: bool) -> bool {
        if query.is_empty() {
            self.anchor = None;
            return false;
        }
        let origin = self.selection().map_or(self.cursor, |(start, _)| start);
        let matches = self.search_ranges(query, case_sensitive);
        let Some((start, end)) = matches
            .iter()
            .copied()
            .find(|(start, _)| *start >= origin)
            .or_else(|| matches.first().copied())
        else {
            self.anchor = None;
            return false;
        };
        self.anchor = Some(start);
        self.cursor = end;
        self.preferred_col = None;
        true
    }

    pub fn search_status(&self, query: &str, case_sensitive: bool) -> (usize, usize) {
        let matches = self.search_ranges(query, case_sensitive);
        let current = self.selection().and_then(|selection| {
            matches
                .iter()
                .position(|range| *range == selection)
                .map(|index| index + 1)
        });
        (current.unwrap_or(0), matches.len())
    }

    pub fn replace_search_selection(
        &mut self,
        query: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        if !self
            .search_ranges(query, case_sensitive)
            .contains(&(start, end))
        {
            return false;
        }
        self.insert(replacement);
        true
    }

    pub fn replace_all_search(
        &mut self,
        query: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> usize {
        if self.read_only {
            return 0;
        }
        let matches = self.search_ranges(query, case_sensitive);
        if matches.is_empty() {
            return 0;
        }
        self.checkpoint();
        for (start, end) in matches.iter().rev() {
            self.text.remove(*start..*end);
            self.text.insert(*start, replacement);
        }
        self.cursor = matches[0].0 + replacement.chars().count();
        self.anchor = None;
        self.finish_edit();
        matches.len()
    }

    pub fn apply_text_edits(
        &mut self,
        edits: &[(usize, usize, usize, usize, String)],
    ) -> Result<(), &'static str> {
        let ranges = self.validate_text_edits(edits)?;
        let mut cursor = self.cursor;
        for (start, end, text) in ranges.iter().rev() {
            let inserted = text.chars().count();
            if cursor >= *end {
                cursor = cursor - (end - start) + inserted;
            } else if cursor > *start {
                cursor = start + (cursor - start).min(inserted);
            }
        }
        self.replace_ranges(self.revision, &ranges)?;
        self.cursor = self.grapheme_floor(cursor.min(self.text.len_chars()));
        Ok(())
    }

    pub(crate) fn validate_text_edits(
        &self,
        edits: &[(usize, usize, usize, usize, String)],
    ) -> Result<Vec<(usize, usize, String)>, &'static str> {
        if self.read_only {
            return Err("buffer is read-only");
        }
        let mut ranges = edits
            .iter()
            .map(|(line, column, end_line, end_column, text)| {
                Ok((
                    self.utf16_position_to_char(*line, *column)
                        .ok_or("invalid start position")?,
                    self.utf16_position_to_char(*end_line, *end_column)
                        .ok_or("invalid end position")?,
                    text.replace("\r\n", "\n"),
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        ranges.sort_by_key(|range| range.0);
        if ranges.iter().any(|(start, end, _)| start > end)
            || ranges
                .windows(2)
                .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
        {
            return Err("invalid or overlapping edits");
        }
        Ok(ranges)
    }

    fn utf16_position_to_char(&self, line: usize, utf16_column: usize) -> Option<usize> {
        if line >= self.text.len_lines() {
            return None;
        }
        let start = self.text.line_to_char(line);
        let content = self.text.line(line).to_string();
        let mut units = 0;
        let mut chars = 0;
        for character in content.trim_end_matches(['\r', '\n']).chars() {
            if units == utf16_column {
                return Some(start + chars);
            }
            units += character.len_utf16();
            chars += 1;
            if units > utf16_column {
                return None;
            }
        }
        (units == utf16_column).then_some(start + chars)
    }

    pub fn cursor_utf16_position(&self) -> (usize, usize) {
        let (line, _) = self.cursor_line_col();
        (line, self.current_line_prefix().encode_utf16().count())
    }

    pub fn set_cursor_utf16_position(&mut self, line: usize, column: usize) {
        if let Some(position) = self.utf16_position_to_char(line, column) {
            self.begin_move(false);
            self.cursor = self.grapheme_floor(position);
        }
    }

    fn search_ranges(&self, query: &str, case_sensitive: bool) -> Rc<Vec<(usize, usize)>> {
        if let Some(cache) = self.search_cache.borrow().as_ref() {
            if cache.revision == self.revision
                && cache.query == query
                && cache.case_sensitive == case_sensitive
            {
                return Rc::clone(&cache.matches);
            }
        }
        let matches = Rc::new(self.compute_search_ranges(query, case_sensitive));
        *self.search_cache.borrow_mut() = Some(SearchCache {
            revision: self.revision,
            query: query.to_owned(),
            case_sensitive,
            matches: Rc::clone(&matches),
        });
        matches
    }

    fn compute_search_ranges(&self, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let text = self.text.to_string();
        if case_sensitive {
            return text
                .match_indices(query)
                .map(|(start, matched)| {
                    let start = self.text.byte_to_char(start);
                    (start, start + matched.chars().count())
                })
                .collect();
        }
        let mut folded = String::with_capacity(text.len());
        let mut boundaries = Vec::with_capacity(self.text.len_chars() + 1);
        for character in text.chars() {
            boundaries.push(folded.len());
            folded.extend(character.to_lowercase());
        }
        boundaries.push(folded.len());
        let query = query.to_lowercase();
        folded
            .match_indices(&query)
            .filter_map(|(start, matched)| {
                Some((
                    boundaries.binary_search(&start).ok()?,
                    boundaries.binary_search(&(start + matched.len())).ok()?,
                ))
            })
            .collect()
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

    pub fn select_all(&mut self) {
        self.edit_group = None;
        self.anchor = Some(0);
        self.cursor = self.text.len_chars();
        self.preferred_col = None;
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
            crlf: self.crlf,
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
                crlf: self.crlf,
            });
            self.crlf = previous.crlf;
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
                crlf: self.crlf,
            });
            self.crlf = next.crlf;
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
        self.write_to(&path, self.disk_stamp)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = absolute_path(path.as_ref())?;
        if self.path.as_ref().is_some_and(|current| {
            current == &path
                || current
                    .canonicalize()
                    .ok()
                    .zip(path.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b)
        }) {
            return self.save();
        }
        if fs::symlink_metadata(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Save As destination exists",
            ));
        }
        self.save_as_confirmed(path, None)
    }

    pub(crate) fn save_as_confirmed(
        &mut self,
        path: PathBuf,
        expected: Option<FileStamp>,
    ) -> io::Result<()> {
        let path = absolute_path(&path)?;
        self.write_to(&path, expected)?;
        self.path = Some(path);
        Ok(())
    }

    fn write_to(&mut self, path: &Path, expected: Option<FileStamp>) -> io::Result<()> {
        if self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "buffer is read-only",
            ));
        }
        let mut text = self.text.to_string();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        atomic_write(path, text.as_bytes(), expected)?;
        self.saved_content_id = self.content_id;
        self.edit_group = None;
        self.disk_stamp = file_stamp(path)?;
        Ok(())
    }

    pub fn check_external_change(&self) -> io::Result<ExternalChange> {
        let Some(path) = self.path() else {
            return Ok(ExternalChange::None);
        };
        let current = file_stamp(path)?;
        Ok(if current == self.disk_stamp {
            ExternalChange::None
        } else if current.is_none() {
            ExternalChange::Deleted
        } else {
            ExternalChange::Modified
        })
    }

    /// External reloads are one undoable transaction; earlier edits stay recoverable.
    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot reload an untitled buffer",
            )
        })?;
        let (text, crlf, stamp) = read_document(path)?;
        let updated = Rope::from_str(&text);
        if updated != self.text || self.crlf != crlf {
            let prefix = self
                .text
                .chars()
                .zip(updated.chars())
                .take_while(|(a, b)| a == b)
                .count();
            let suffix_limit = self.text.len_chars().min(updated.len_chars()) - prefix;
            let suffix = self
                .text
                .chars_at(self.text.len_chars())
                .reversed()
                .zip(updated.chars_at(updated.len_chars()).reversed())
                .take(suffix_limit)
                .take_while(|(a, b)| a == b)
                .count();
            let old_end = self.text.len_chars() - suffix;
            let new_end = updated.len_chars() - suffix;
            let map = |position: usize| {
                if position < prefix {
                    position
                } else if position >= old_end {
                    new_end + position - old_end
                } else {
                    prefix + (position - prefix).min(new_end - prefix)
                }
            };
            self.checkpoint();
            self.cursor = map(self.cursor);
            self.anchor = self.anchor.map(map);
            self.text = updated;
            self.crlf = crlf;
            self.finish_edit();
            // A change can combine adjacent Unicode code points into one grapheme.
            self.cursor = self.grapheme_floor(self.cursor);
            self.anchor = self.anchor.map(|position| self.grapheme_floor(position));
        }
        self.saved_content_id = self.content_id;
        self.disk_stamp = stamp;
        self.edit_group = None;
        Ok(())
    }

    fn grapheme_floor(&self, position: usize) -> usize {
        let line = self.text.char_to_line(position);
        let start = self.text.line_to_char(line);
        let content = self.text.line(line).to_string();
        let mut boundary = start;
        for grapheme in content.graphemes(true) {
            let next = boundary + grapheme.chars().count();
            if next > position {
                break;
            }
            boundary = next;
        }
        boundary
    }

    pub fn keep_after_external_change(&mut self) {
        self.disk_stamp = self
            .path
            .as_deref()
            .and_then(|path| file_stamp(path).ok().flatten());
        if !self.is_dirty() {
            self.content_id = self.next_content_id;
            self.next_content_id = self.next_content_id.wrapping_add(1);
        }
        self.edit_group = None;
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
    fn search_supports_direction_counts_and_case_sensitivity() {
        let mut buffer = Buffer::from_text("One one ONE".into(), None, false);
        assert_eq!(buffer.find_search("one", false, false), Some((1, 3)));
        assert_eq!(buffer.find_search("one", false, false), Some((2, 3)));
        assert_eq!(buffer.find_search("one", false, true), Some((1, 3)));
        assert_eq!(buffer.search_status("one", false), (1, 3));
        assert_eq!(buffer.search_status("one", true), (0, 1));
        assert_eq!(buffer.search_status("a very long query", false), (0, 0));
    }

    #[test]
    fn search_replace_and_replace_all_are_undoable() {
        let mut buffer = Buffer::from_text("one ONE one".into(), None, false);
        buffer.find_search("one", false, false);
        assert!(buffer.replace_search_selection("one", "two", false));
        assert_eq!(buffer.text(), "two ONE one");
        let replaced = buffer.replace_all_search("one", "three", false);
        assert_eq!(replaced, 2);
        assert_eq!(buffer.text(), "two three three");
        buffer.undo();
        assert_eq!(buffer.text(), "two ONE one");
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

    #[test]
    fn virtual_buffer_has_a_name_and_is_read_only() {
        let buffer = Buffer::read_only("Workspace.diff", "diff content\n".into());
        assert_eq!(buffer.name(), "Workspace.diff");
        assert!(buffer.is_read_only());
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.path(), None);
    }

    #[test]
    fn lsp_text_edits_use_utf16_columns_and_are_atomic() {
        let mut buffer = Buffer::from_text("a😀b\nsecond\n".into(), None, false);
        buffer
            .apply_text_edits(&[(0, 1, 0, 3, "X".into()), (1, 0, 1, 6, "line".into())])
            .unwrap();
        assert_eq!(buffer.text(), "aXb\nline\n");
        buffer.undo();
        assert_eq!(buffer.text(), "a😀b\nsecond\n");
    }

    #[test]
    fn range_edit_rejects_stale_revisions() {
        let mut buffer = Buffer::from_text("hello".into(), None, false);
        assert_eq!(buffer.replace_range(0, 0, 5, "world"), Ok(1));
        assert_eq!(
            buffer.replace_range(0, 0, 1, "x"),
            Err("stale buffer revision")
        );
        assert_eq!(buffer.text(), "world");
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn replace_all_does_not_overlap_matches() {
        let mut buffer = Buffer::empty();
        buffer.insert("aaa AAA");
        assert_eq!(buffer.replace_all_search("aa", "", false), 2);
        assert_eq!(buffer.text(), "a A");
        buffer.undo();
        assert_eq!(buffer.text(), "aaa AAA");
    }
    #[test]
    fn overlapping_edits_are_rejected_before_mutation() {
        let mut buffer = Buffer::empty();
        buffer.insert("abcdef");
        let revision = buffer.revision();
        assert!(buffer
            .replace_ranges(revision, &[(0, 5, "".into()), (3, 6, "".into())])
            .is_err());
        assert_eq!(buffer.text(), "abcdef");
        assert_eq!(buffer.revision(), revision);
    }
    #[test]
    fn save_preserves_permissions_and_symlink_target() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("script.sh");
        let link = directory.path().join("link.sh");
        fs::write(&target, "before").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&target, &link).unwrap();
        let mut buffer = Buffer::open(&link).unwrap();
        buffer.select_all();
        buffer.insert("after");
        buffer.save().unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "after");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::metadata(link).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    #[test]
    fn save_never_uses_predictable_temporary_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        let victim = directory.path().join("victim");
        fs::write(&path, "before").unwrap();
        fs::write(&victim, "private").unwrap();
        symlink(&victim, path.with_extension("txt.tted-tmp")).unwrap();
        let mut buffer = Buffer::open(path).unwrap();
        buffer.insert("after");
        buffer.save().unwrap();
        assert_eq!(fs::read_to_string(victim).unwrap(), "private");
    }
    #[test]
    fn save_checks_external_change_without_polling() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        fs::write(&path, "initial").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        buffer.insert("human");
        fs::write(&path, "external").unwrap();
        assert_eq!(buffer.save().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        assert!(buffer.is_dirty());
        buffer.keep_after_external_change();
        buffer.save().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "humaninitial");
    }
    #[test]
    fn save_as_requires_explicit_unchanged_destination() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("exists");
        fs::write(&path, "original").unwrap();
        let mut buffer = Buffer::empty();
        buffer.insert("new");
        assert_eq!(
            buffer.save_as(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let stamp = file_stamp(&path).unwrap();
        fs::write(&path, "changed while confirming").unwrap();
        assert_eq!(
            buffer
                .save_as_confirmed(path.clone(), stamp)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "changed while confirming"
        );
        assert!(buffer.path().is_none());
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;
    #[test]
    fn external_reload_preserves_cursor_selection_and_earlier_undo() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "alpha\nbeta\n").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        buffer.insert("human\n");
        buffer.save().unwrap();
        buffer.set_cursor_line_col(2, 0, false);
        buffer.move_horizontal(1, true);
        fs::write(&path, "external\nhuman\nalpha\nbeta\n").unwrap();
        buffer.reload_from_disk().unwrap();
        assert_eq!(buffer.cursor_line_col(), (3, 1));
        assert_eq!(buffer.selected_text().as_deref(), Some("b"));
        assert!(!buffer.is_dirty());
        buffer.undo();
        assert_eq!(buffer.text(), "human\nalpha\nbeta\n");
        assert_eq!(buffer.cursor_line_col(), (2, 1));
        assert!(buffer.is_dirty());
        buffer.redo();
        assert!(!buffer.is_dirty());
        buffer.undo();
        buffer.undo();
        assert_eq!(buffer.text(), "alpha\nbeta\n");
    }
    #[test]
    fn reload_keeps_unsaved_text_recoverable_and_restores_line_endings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "original\r\n").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        buffer.insert("unsaved ");
        fs::write(&path, "external\n").unwrap();
        buffer.reload_from_disk().unwrap();
        assert!(!buffer.is_dirty());
        buffer.undo();
        assert_eq!(buffer.text(), "unsaved original\n");
        buffer.save().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "unsaved original\r\n");
    }
    #[test]
    fn identical_external_rewrite_does_not_add_an_undo_step() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "original").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        buffer.insert("edit ");
        buffer.save().unwrap();
        fs::write(&path, "edit original").unwrap();
        buffer.reload_from_disk().unwrap();
        buffer.undo();
        assert_eq!(buffer.text(), "original");
    }
    #[test]
    fn reload_cursor_stays_on_grapheme_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "ab").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        buffer.move_horizontal(1, false);
        fs::write(&path, "a\u{301}b").unwrap();
        buffer.reload_from_disk().unwrap();
        assert_eq!(buffer.cursor(), 2);
        buffer.backspace();
        assert_eq!(buffer.text(), "b");
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn unicode_case_search_maps_expanding_lowercase_to_original_ranges() {
        let mut buffer = Buffer::empty();
        buffer.insert("İstanbul İSTANBUL");
        assert_eq!(
            buffer.replace_all_search("i\u{307}stanbul", "city", false),
            2
        );
        assert_eq!(buffer.text(), "city city");
        buffer.undo();
        assert_eq!(buffer.search_status("İSTANBUL", false).1, 2);
    }
    #[test]
    fn cached_search_refreshes_after_edit_undo_and_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "match").unwrap();
        let mut buffer = Buffer::open(&path).unwrap();
        assert_eq!(buffer.search_status("match", true).1, 1);
        buffer.insert("match ");
        assert_eq!(buffer.search_status("match", true).1, 2);
        buffer.undo();
        assert_eq!(buffer.search_status("match", true).1, 1);
        fs::write(&path, "none").unwrap();
        buffer.reload_from_disk().unwrap();
        assert_eq!(buffer.search_status("match", true).1, 0);
    }
}

#[cfg(test)]
mod language_position_tests {
    use super::*;
    #[test]
    fn unicode_positions_and_invalid_batches_are_safe() {
        let mut buffer = Buffer::empty();
        buffer.insert("a😀b\nline");
        buffer.set_cursor_line_col(0, 2, false);
        assert_eq!(buffer.cursor_utf16_position(), (0, 3));
        buffer.set_cursor_utf16_position(0, 3);
        assert_eq!(buffer.cursor_line_col(), (0, 2));
        let before = buffer.text();
        let revision = buffer.revision();
        assert!(buffer
            .apply_text_edits(&[(0, 0, 0, 1, "x".into()), (0, 2, 0, 3, "y".into())])
            .is_err());
        assert_eq!(buffer.text(), before);
        assert_eq!(buffer.revision(), revision);
        assert!(buffer
            .apply_text_edits(&[(0, 0, 0, 3, "x".into()), (0, 1, 0, 4, "y".into())])
            .is_err());
        assert_eq!(buffer.text(), before);
    }
}
