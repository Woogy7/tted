use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskMarker {
    pub rendered_line: usize,
    pub rendered_column: usize,
    pub source_marker_char: usize,
    pub checked: bool,
}

#[derive(Clone, Debug)]
struct SourceRun {
    source_start: usize,
    source_end: usize,
    exact: bool,
}

pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
    pub tasks: Vec<TaskMarker>,
    runs: Vec<Vec<SourceRun>>,
    pub(crate) code_rows: Vec<bool>,
    pub(crate) fenced_rows: Vec<bool>,
    pub(crate) fence_starts: Vec<bool>,
    fenced_blocks: Vec<(usize, usize)>,
    line_starts: Vec<usize>,
}

impl RenderedMarkdown {
    pub(crate) fn same_fenced_block(&self, row: usize, cursor_row: usize) -> bool {
        let count = self
            .fenced_blocks
            .partition_point(|(start, _)| *start <= cursor_row);
        count.checked_sub(1).is_some_and(|index| {
            let (start, end) = self.fenced_blocks[index];
            cursor_row <= end && (start..=end).contains(&row)
        })
    }

    pub(crate) fn source_line_styles(&self, row: usize, length: usize) -> Vec<Style> {
        let mut styles = vec![Style::default().fg(theme::SUBTEXT0); length];
        let Some(line) = self.lines.get(row) else {
            return styles;
        };
        let start = self.line_starts[row];
        // Fill whole runs once instead of searching every span for every character.
        for (span, run) in line.spans.iter().zip(&self.runs[row]).rev() {
            let from = run.source_start.saturating_sub(start).min(length);
            let to = run.source_end.saturating_sub(start).min(length);
            if from < to {
                styles[from..to].fill(span.style);
            }
        }
        styles
    }

    pub fn source_style(&self, row: usize, position: usize) -> Style {
        self.lines
            .get(row)
            .into_iter()
            .flat_map(|line| line.spans.iter())
            .zip(self.runs.get(row).into_iter().flatten())
            .find_map(|(span, run)| {
                (position >= run.source_start && position < run.source_end).then_some(span.style)
            })
            .unwrap_or_else(|| Style::default().fg(theme::SUBTEXT0))
    }

    /// Map a click in formatted text back to a source character column.
    pub fn source_column(&self, row: usize, column: usize) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        let Some(line) = self.lines.get(row) else {
            return 0;
        };
        let start = self.line_starts[row];
        let mut screen = 0;
        for (span, run) in line.spans.iter().zip(&self.runs[row]) {
            let width = UnicodeWidthStr::width(span.content.as_ref());
            if column < screen + width {
                if !run.exact {
                    return if column - screen < width / 2 {
                        run.source_start
                    } else {
                        run.source_end
                    }
                    .saturating_sub(start);
                }
                let mut offset = 0;
                for grapheme in span.content.graphemes(true) {
                    let width = UnicodeWidthStr::width(grapheme);
                    if screen + width > column {
                        break;
                    }
                    screen += width;
                    offset += grapheme.chars().count();
                }
                return run.source_start + offset - start;
            }
            screen += width;
        }
        self.runs[row]
            .last()
            .map_or(0, |run| run.source_end.saturating_sub(start))
    }
}

pub fn render(source: &str) -> Vec<Line<'static>> {
    render_document(source).lines
}

/// Keep one display row per source line. The editor can reveal any active line
/// without moving the surrounding document or losing source coordinates.
pub fn render_document(source: &str) -> RenderedMarkdown {
    let mut byte_starts = vec![0];
    let mut char_starts = vec![0];
    let mut char_bytes = Vec::new();
    for (index, (byte, character)) in source.char_indices().enumerate() {
        char_bytes.push(byte);
        if character == '\n' {
            byte_starts.push(byte + 1);
            char_starts.push(index + 1);
        }
    }
    let mut document = RenderedMarkdown {
        lines: vec![Line::default(); byte_starts.len()],
        tasks: Vec::new(),
        runs: vec![Vec::new(); byte_starts.len()],
        code_rows: vec![false; byte_starts.len()],
        fenced_rows: vec![false; byte_starts.len()],
        fence_starts: vec![false; byte_starts.len()],
        fenced_blocks: Vec::new(),
        line_starts: char_starts.clone(),
    };
    char_bytes.push(source.len());
    let char_offset = |byte: usize| {
        char_bytes
            .binary_search(&byte)
            .expect("parser source offsets are UTF-8 boundaries")
    };
    let row_at = |byte: usize| {
        byte_starts
            .partition_point(|start| *start <= byte)
            .saturating_sub(1)
    };
    let mut style = Style::default();
    let mut styles = Vec::new();
    let mut lists = Vec::<Option<u64>>::new();
    let append = |document: &mut RenderedMarkdown,
                  row: usize,
                  text: String,
                  style: Style,
                  source_start: usize,
                  source_end: usize,
                  exact: bool| {
        if text.is_empty() {
            return;
        }
        document.lines[row].spans.push(Span::styled(text, style));
        document.runs[row].push(SourceRun {
            source_start,
            source_end,
            exact,
        });
    };
    for (event, range) in Parser::new_ext(source, Options::all()).into_offset_iter() {
        let row = row_at(range.start);
        match event {
            Event::Start(tag) => {
                styles.push(style);
                match tag {
                    Tag::Heading { level, .. } => style = heading_style(level),
                    Tag::Emphasis => style = style.add_modifier(Modifier::ITALIC),
                    Tag::Strong => style = style.add_modifier(Modifier::BOLD),
                    Tag::Strikethrough => style = style.add_modifier(Modifier::CROSSED_OUT),
                    Tag::Link { .. } => {
                        style = style.fg(theme::BLUE).add_modifier(Modifier::UNDERLINED)
                    }
                    Tag::CodeBlock(kind) => {
                        let last = row_at(range.end.saturating_sub(1));
                        document.code_rows[row..=last].fill(true);
                        if matches!(kind, pulldown_cmark::CodeBlockKind::Fenced(_)) {
                            document.fenced_rows[row..=last].fill(true);
                            document.fence_starts[row] = true;
                            document.fenced_blocks.push((row, last));
                        }
                        style = Style::default().fg(theme::GREEN).bg(theme::SURFACE0)
                    }
                    Tag::List(start) => lists.push(start),
                    Tag::Item => {
                        let indent = "  ".repeat(lists.len().saturating_sub(1));
                        let bullet = match lists.last_mut() {
                            Some(Some(number)) => {
                                let label = format!("{number}. ");
                                *number += 1;
                                label
                            }
                            _ => "• ".into(),
                        };
                        let start = char_offset(range.start);
                        append(
                            &mut document,
                            row,
                            format!("{indent}{bullet}"),
                            style,
                            start,
                            start,
                            false,
                        );
                    }
                    Tag::BlockQuote(_) => {
                        let start = char_offset(range.start);
                        append(
                            &mut document,
                            row,
                            "│ ".into(),
                            Style::default().fg(theme::OVERLAY0),
                            start,
                            start,
                            false,
                        );
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                if matches!(tag, TagEnd::List(_)) {
                    lists.pop();
                }
                if matches!(tag, TagEnd::TableCell) {
                    let end = char_offset(range.end);
                    append(
                        &mut document,
                        row,
                        " │ ".into(),
                        Style::default().fg(theme::OVERLAY0),
                        end,
                        end,
                        false,
                    );
                }
                style = styles.pop().unwrap_or_default();
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                let exact = text.as_ref() == &source[range.clone()];
                let mut byte = range.start;
                for (line_offset, part) in text.split_inclusive('\n').enumerate() {
                    let row = if exact {
                        row_at(byte)
                    } else {
                        (row + line_offset).min(byte_starts.len() - 1)
                    };
                    if !exact && line_offset > 0 {
                        byte = byte_starts[row];
                    }
                    let content = part.trim_end_matches(['\r', '\n']);
                    let end = if exact {
                        byte + content.len()
                    } else {
                        range
                            .end
                            .min(byte_starts.get(row + 1).copied().unwrap_or(source.len()))
                    };
                    append(
                        &mut document,
                        row,
                        content.into(),
                        style,
                        char_offset(byte),
                        char_offset(end),
                        exact,
                    );
                    if exact {
                        byte += part.len();
                    }
                }
            }
            Event::Code(text) => {
                let original = &source[range.clone()];
                let found = original.find(text.as_ref());
                let start = range.start + found.unwrap_or(0);
                let end = if found.is_some() {
                    start + text.len()
                } else {
                    range.end
                };
                append(
                    &mut document,
                    row,
                    text.into_string(),
                    Style::default().fg(theme::PEACH).bg(theme::SURFACE0),
                    char_offset(start),
                    char_offset(end),
                    found.is_some(),
                );
            }
            Event::Rule => {
                append(
                    &mut document,
                    row,
                    "─".repeat(40),
                    Style::default().fg(theme::OVERLAY0),
                    char_offset(range.start),
                    char_offset(range.end),
                    false,
                );
            }
            Event::TaskListMarker(done) => {
                if let Some(marker) = source[range.clone()].find([' ', 'x', 'X']) {
                    document.tasks.push(TaskMarker {
                        rendered_line: row,
                        rendered_column: document.lines[row].width(),
                        source_marker_char: char_offset(range.start + marker),
                        checked: done,
                    });
                }
                append(
                    &mut document,
                    row,
                    if done { "[x] ".into() } else { "[ ] ".into() },
                    style,
                    char_offset(range.start),
                    char_offset(range.end),
                    false,
                );
            }
            _ => {}
        }
    }
    document
}

fn heading_style(level: HeadingLevel) -> Style {
    let color = match level {
        HeadingLevel::H1 => theme::MAUVE,
        HeadingLevel::H2 => theme::SAPPHIRE,
        _ => theme::BLUE,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_markdown_structure() {
        let lines = render("# Title\n\n- one\n- **two**\n");
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Title"));
        assert!(text.contains("• one"));
        assert!(text.contains("• two"));
    }

    #[test]
    fn task_markers_retain_rendered_and_unicode_source_positions() {
        let source = "é\n\n- [ ] first\n- [x] second\n";
        let document = render_document(source);
        assert_eq!(document.tasks.len(), 2);
        assert_eq!(document.tasks[0].rendered_column, 2);
        assert_eq!(
            source.chars().nth(document.tasks[0].source_marker_char),
            Some(' ')
        );
        assert!(!document.tasks[0].checked);
        assert!(document.tasks[1].checked);
    }
}

#[derive(Default)]
pub(crate) struct MarkdownCache {
    document: Option<(u64, u64, std::rc::Rc<RenderedMarkdown>)>,
}
impl MarkdownCache {
    pub fn get(&mut self, buffer: &crate::buffer::Buffer) -> std::rc::Rc<RenderedMarkdown> {
        if let Some((id, revision, document)) = &self.document {
            if *id == buffer.id() && *revision == buffer.revision() {
                return std::rc::Rc::clone(document);
            }
        }
        let document = std::rc::Rc::new(render_document(&buffer.text()));
        self.document = Some((
            buffer.id(),
            buffer.revision(),
            std::rc::Rc::clone(&document),
        ));
        document
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    fn source_rows_and_clicks_preserve_inline_unicode_and_fenced_code() {
        let source =
            "# Heading\n\n**🌍 bold** and `code`\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n";
        let document = render_document(source);
        assert_eq!(document.lines.len(), source.split('\n').count());
        assert_eq!(document.lines[2].to_string(), "🌍 bold and code");
        assert_eq!(document.source_column(2, 0), 2);
        assert_eq!(document.source_column(2, 2), 3);
        assert_eq!(document.lines[5].to_string(), "let a = 1;");
        assert_eq!(document.lines[6].to_string(), "let b = 2;");
    }
    #[test]
    fn nested_styles_restore_heading_and_outer_emphasis() {
        let document = render_document("# title **bold** after\n");
        assert!(document.lines[0]
            .spans
            .last()
            .unwrap()
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }
    #[test]
    fn navigation_reuses_cached_markdown_and_edits_invalidate_it() {
        let mut buffer = crate::buffer::Buffer::empty();
        buffer.insert("# title\n");
        let mut cache = MarkdownCache::default();
        let first = cache.get(&buffer);
        buffer.move_horizontal(-1, false);
        assert!(std::rc::Rc::ptr_eq(&first, &cache.get(&buffer)));
        buffer.insert("new");
        assert!(!std::rc::Rc::ptr_eq(&first, &cache.get(&buffer)));
    }
}

#[cfg(test)]
mod active_line_tests {
    use super::*;
    #[test]
    fn active_source_text_keeps_live_heading_and_emphasis_styles() {
        let document = render_document("# Title\n**bold**\n");
        assert_eq!(document.source_style(0, 0).fg, Some(theme::SUBTEXT0));
        assert!(document
            .source_style(0, 2)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(document
            .source_style(1, 10)
            .add_modifier
            .contains(Modifier::BOLD));
    }
}

#[cfg(test)]
mod source_style_tests {
    use super::*;
    #[test]
    fn bulk_source_styles_match_preview_for_unicode_nested_markup_and_code() {
        let source = "# 🌍 **bold** and *italic*\n> - [x] a &amp; b\n\n```rust\nlet x = 1;\n```\n";
        let document = render_document(source);
        let mut start = 0;
        for (row, text) in source.split_inclusive('\n').enumerate() {
            let length = text.chars().count();
            let styles = document.source_line_styles(row, length);
            for (offset, style) in styles.into_iter().enumerate() {
                assert_eq!(style, document.source_style(row, start + offset));
            }
            start += length;
        }
    }
}
