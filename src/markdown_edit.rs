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
pub(crate) fn newline(buffer: &mut Buffer, soft: bool) -> bool {
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
    } else if line[prefix.content..].trim().is_empty() {
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
            assert!(newline(&mut buffer, false), "{source}");
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
        assert!(newline(&mut buffer, false));
        assert_eq!(buffer.text(), "- 🌍 \n- hello");
        buffer.undo();
        buffer.move_line_edge(true, false);
        assert!(newline(&mut buffer, true));
        assert_eq!(buffer.text(), "- 🌍 hello  \n  ");
    }
    #[test]
    fn ignores_nonlists_and_cursor_inside_marker() {
        for source in ["---", "* * *", "plain", "1.no", "1234567890. no"] {
            let mut buffer = Buffer::empty();
            buffer.insert(source);
            assert!(!newline(&mut buffer, false));
        }
        let mut buffer = Buffer::empty();
        buffer.insert("- item");
        buffer.set_cursor_line_col(0, 1, false);
        assert!(!newline(&mut buffer, false));
    }
}
