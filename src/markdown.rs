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

pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
    pub tasks: Vec<TaskMarker>,
}

pub fn render(source: &str) -> Vec<Line<'static>> {
    render_document(source).lines
}

pub fn render_document(source: &str) -> RenderedMarkdown {
    let mut lines = Vec::new();
    let mut tasks = Vec::new();
    let mut current = Vec::new();
    let mut style = Style::default();
    let mut list_depth = 0usize;

    let finish = |lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>| {
        lines.push(Line::from(std::mem::take(current)));
    };

    for (event, source_range) in Parser::new_ext(source, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                style = heading_style(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                finish(&mut lines, &mut current);
                lines.push(Line::default());
                style = Style::default();
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                finish(&mut lines, &mut current);
                lines.push(Line::default());
            }
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                if list_depth == 0 {
                    lines.push(Line::default());
                }
            }
            Event::Start(Tag::Item) => current.push(Span::raw(format!(
                "{}• ",
                "  ".repeat(list_depth.saturating_sub(1))
            ))),
            Event::End(TagEnd::Item) => finish(&mut lines, &mut current),
            Event::Start(Tag::BlockQuote(_)) => {
                current.push(Span::styled("│ ", Style::default().fg(theme::OVERLAY0)))
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                finish(&mut lines, &mut current);
                lines.push(Line::default());
            }
            Event::Start(Tag::Emphasis) => style = style.add_modifier(Modifier::ITALIC),
            Event::End(TagEnd::Emphasis) => style = style.remove_modifier(Modifier::ITALIC),
            Event::Start(Tag::Strong) => style = style.add_modifier(Modifier::BOLD),
            Event::End(TagEnd::Strong) => style = style.remove_modifier(Modifier::BOLD),
            Event::Start(Tag::CodeBlock(_)) => {
                style = Style::default().fg(theme::GREEN).bg(theme::SURFACE0)
            }
            Event::End(TagEnd::CodeBlock) => {
                finish(&mut lines, &mut current);
                lines.push(Line::default());
                style = Style::default();
            }
            Event::Text(text) => current.push(Span::styled(text.into_string(), style)),
            Event::Code(text) => current.push(Span::styled(
                text.into_string(),
                Style::default().fg(theme::PEACH).bg(theme::SURFACE0),
            )),
            Event::SoftBreak | Event::HardBreak => finish(&mut lines, &mut current),
            Event::Rule => {
                finish(&mut lines, &mut current);
                lines.push(Line::styled(
                    "─".repeat(40),
                    Style::default().fg(theme::OVERLAY0),
                ));
            }
            Event::TaskListMarker(done) => {
                let rendered_column = current
                    .iter()
                    .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
                    .sum();
                if let Some(relative_marker) =
                    source[source_range.clone()]
                        .char_indices()
                        .find_map(|(index, character)| {
                            matches!(character, ' ' | 'x' | 'X').then_some(index)
                        })
                {
                    tasks.push(TaskMarker {
                        rendered_line: lines.len(),
                        rendered_column,
                        source_marker_char: source[..source_range.start + relative_marker]
                            .chars()
                            .count(),
                        checked: done,
                    });
                }
                current.push(Span::raw(if done { "[x] " } else { "[ ] " }))
            }
            Event::Html(html) | Event::InlineHtml(html) => current.push(Span::styled(
                html.into_string(),
                Style::default().fg(theme::OVERLAY0),
            )),
            _ => {}
        }
    }
    if !current.is_empty() {
        finish(&mut lines, &mut current);
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    RenderedMarkdown { lines, tasks }
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
