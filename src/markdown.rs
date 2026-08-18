use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme;

pub fn render(source: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut style = Style::default();
    let mut list_depth = 0usize;

    let finish = |lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>| {
        lines.push(Line::from(std::mem::take(current)));
    };

    for event in Parser::new_ext(source, Options::all()) {
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
    lines
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
}
