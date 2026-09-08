//! Source edits shared by Markdown source and live preview.
use crate::buffer::Buffer;

struct Prefix<'a> {
    container: &'a str,
    content: usize,
    next: String,
    list: bool,
}

fn prefix(line: &str) -> Option<Prefix<'_>> {
    let compact = line
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    if compact.len() >= 3
        && b"-*_"
            .iter()
            .any(|marker| compact.bytes().all(|c| c == *marker))
    {
        return None;
    }
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut start = indent;
    while line[start..].starts_with('>') {
        start += 1;
        if line[start..].starts_with(' ') {
            start += 1;
        }
    }
    let rest = &line[start..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let marker = if matches!(rest.as_bytes().first(), Some(b'-' | b'+' | b'*')) {
        Some((1, rest[..1].to_owned()))
    } else if (1..=9).contains(&digits) && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
    {
        Some((
            digits + 1,
            format!(
                "{}{}",
                rest[..digits].parse::<u64>().ok()? + 1,
                &rest[digits..digits + 1]
            ),
        ))
    } else {
        None
    };
    if let Some((length, next)) = marker {
        let tail = &rest[length..];
        if tail.starts_with([' ', '\t']) {
            let whitespace = tail.len() - tail.trim_start_matches([' ', '\t']).len();
            let mut content = start + length + whitespace;
            let mut next = format!("{}{next} ", &line[..start]);
            let task = &line[content..];
            if (task.starts_with("[ ]") || task.starts_with("[x]") || task.starts_with("[X]"))
                && (task.len() == 3 || task[3..].starts_with([' ', '\t']))
            {
                content += 3;
                content +=
                    line[content..].len() - line[content..].trim_start_matches([' ', '\t']).len();
                next.push_str("[ ] ");
            }
            return Some(Prefix {
                container: &line[..start],
                content,
                next,
                list: true,
            });
        }
    }
    (start > indent).then(|| Prefix {
        container: &line[..indent],
        content: start,
        next: line[..start].into(),
        list: false,
    })
}

pub(crate) fn is_list(line: &str) -> bool {
    prefix(line).is_some_and(|prefix| prefix.list)
}

/// Return false when ordinary code-editor indentation should handle Enter.
pub(crate) fn newline(buffer: &mut Buffer, soft: bool, exit_empty: bool) -> bool {
    if buffer.selection().is_some() {
        return false;
    }
    let (row, column) = buffer.cursor_line_col();
    let line = buffer.line(row);
    let line = line.trim_end_matches(['\r', '\n']);
    let Some(prefix) = prefix(line) else {
        return false;
    };
    let before = buffer.current_line_prefix();
    if before.len() < prefix.content {
        return false;
    }
    if soft {
        let indent = if prefix.list {
            format!(
                "{}{}",
                &line[..prefix.container.len()],
                " ".repeat(prefix.content - prefix.container.len())
            )
        } else {
            prefix.next
        };
        buffer.insert(&format!("  \n{indent}"));
    } else if exit_empty && line[prefix.content..].trim().is_empty() {
        let start = buffer.line_start_char(row) + prefix.container.chars().count();
        let end = buffer.line_start_char(row) + line.chars().count();
        let revision = buffer.revision();
        // Leave the surrounding quote/indent in place and remove the empty marker.
        buffer
            .replace_range(revision, start, end, "")
            .expect("valid Markdown prefix");
    } else {
        debug_assert!(column >= prefix.container.chars().count());
        buffer.insert(&format!("\n{}", prefix.next));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continues_lists_tasks_quotes_and_exits_empty_items() {
        for (source, expected) in [
            ("- hello", "- hello\n- "),
            ("* hello", "* hello\n* "),
            ("9) hello", "9) hello\n10) "),
            ("  3. hello", "  3. hello\n  4. "),
            ("- [x] done", "- [x] done\n- [ ] "),
            ("> - hello", "> - hello\n> - "),
            ("> quote", "> quote\n> "),
            ("- ", ""),
            ("- [ ] ", ""),
            ("> - ", "> "),
            ("> ", ""),
        ] {
            let mut buffer = Buffer::empty();
            buffer.insert(source);
            assert!(newline(&mut buffer, false, true), "{source}");
            assert_eq!(buffer.text(), expected);
            buffer.undo();
            assert_eq!(buffer.text(), source);
        }
    }
    #[test]
    fn splits_unicode_items_and_supports_continuation_lines() {
        let mut buffer = Buffer::empty();
        buffer.insert("- 🌍 hello");
        buffer.set_cursor_line_col(0, 4, false);
        assert!(newline(&mut buffer, false, true));
        assert_eq!(buffer.text(), "- 🌍 \n- hello");
        buffer.undo();
        buffer.move_line_edge(true, false);
        assert!(newline(&mut buffer, true, true));
        assert_eq!(buffer.text(), "- 🌍 hello  \n  ");
    }
    #[test]
    fn ignores_nonlists_and_cursor_inside_marker() {
        for source in ["---", "* * *", "plain", "1.no", "1234567890. no"] {
            let mut buffer = Buffer::empty();
            buffer.insert(source);
            assert!(!newline(&mut buffer, false, true));
        }
        let mut buffer = Buffer::empty();
        buffer.insert("- item");
        buffer.set_cursor_line_col(0, 1, false);
        assert!(!newline(&mut buffer, false, true));
    }
}

pub(crate) fn backtick(buffer: &mut Buffer, in_code: bool) {
    if let Some(selected) = buffer.selected_text() {
        let longest = selected
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let delimiter = "`".repeat(longest + 1);
        let padding = if selected.starts_with('`') || selected.ends_with('`') {
            " "
        } else {
            ""
        };
        buffer.insert(&format!(
            "{delimiter}{padding}{selected}{padding}{delimiter}"
        ));
        return;
    }
    let before = buffer.current_line_prefix();
    // A third consecutive backtick expands the empty inline pair into a fence pair.
    if before.trim_start_matches([' ', '\t']) == "``"
        && matches!(buffer.char_at_cursor(), None | Some('\n'))
    {
        buffer.insert("````");
        buffer.move_horizontal(-3, false);
    } else if buffer.char_at_cursor() == Some('`') {
        buffer.move_horizontal(1, false);
    } else if in_code || before.ends_with('\\') {
        buffer.insert_typed("`");
    } else {
        buffer.insert("``");
        buffer.move_horizontal(-1, false);
    }
}

/// Split a paired fence or complete a new opening fence on Enter.
pub(crate) fn fence_newline(buffer: &mut Buffer, opening_fence: bool) -> bool {
    if buffer.selection().is_some() {
        return false;
    }
    let before = buffer.current_line_prefix();
    let opening = before.trim_start_matches(' ');
    let indent_len = before.len() - opening.len();
    if indent_len > 3 || !opening.starts_with("```") {
        return false;
    }
    let count = opening.bytes().take_while(|c| *c == b'`').count();
    if opening[count..].contains('`') {
        return false;
    }
    let (row, _) = buffer.cursor_line_col();
    let line = buffer.line(row);
    let tail = line[before.len()..].trim_end_matches(['\r', '\n']);
    let fence = "`".repeat(count);
    let indent = &before[..indent_len];
    if tail == fence {
        buffer.insert(&format!("\n{indent}\n{indent}"));
        buffer.move_horizontal(-((indent_len + 1) as isize), false);
        return true;
    }
    if !opening_fence || !tail.is_empty() {
        return false;
    }
    // Reuse an existing closing fence instead of inserting a duplicate.
    for next in row + 1..buffer.len_lines() {
        let line = buffer.line(next);
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3
            && trimmed.starts_with(&fence)
            && trimmed
                .trim_end_matches(['\r', '\n', ' ', '\t'])
                .bytes()
                .all(|c| c == b'`')
        {
            return false;
        }
    }
    buffer.insert(&format!("\n{indent}\n{indent}{fence}"));
    buffer.move_horizontal(-((indent_len + count + 1) as isize), false);
    true
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    #[test]
    fn inline_backticks_pair_skip_and_wrap_unicode() {
        let mut buffer = Buffer::empty();
        backtick(&mut buffer, false);
        assert_eq!((buffer.text(), buffer.cursor()), ("``".into(), 1));
        buffer.insert_typed("🌍");
        backtick(&mut buffer, false);
        assert_eq!((buffer.text(), buffer.cursor()), ("`🌍`".into(), 3));
        buffer.select_all();
        backtick(&mut buffer, false);
        assert_eq!(buffer.text(), "`` `🌍` ``");
        buffer.undo();
        assert_eq!(buffer.text(), "`🌍`");
    }
    #[test]
    fn triple_backticks_complete_fence_with_language_and_undo() {
        let mut buffer = Buffer::empty();
        for _ in 0..3 {
            backtick(&mut buffer, false);
        }
        assert_eq!((buffer.text(), buffer.cursor()), ("``````".into(), 3));
        buffer.insert_typed("rust");
        assert!(fence_newline(&mut buffer, false));
        assert_eq!(buffer.text(), "```rust\n\n```");
        assert_eq!(buffer.cursor_line_col(), (1, 0));
        buffer.undo();
        assert_eq!(buffer.text(), "```rust```");
    }
    #[test]
    fn pasted_fence_completes_but_existing_closer_is_preserved() {
        let mut buffer = Buffer::empty();
        buffer.insert("```rust");
        assert!(fence_newline(&mut buffer, true));
        assert_eq!(buffer.text(), "```rust\n\n```");
        buffer.set_cursor_line_col(0, 7, false);
        assert!(!fence_newline(&mut buffer, true));
        buffer.set_cursor_line_col(2, 3, false);
        assert!(!fence_newline(&mut buffer, false));
    }
}
