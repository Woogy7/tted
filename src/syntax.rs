//! Incremental syntax state and visible-line cache, independent of editor input.
use crate::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ropey::Rope;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, HighlightState, Theme},
    parsing::{ParseState, SyntaxSet},
};

#[derive(Default)]
pub(crate) struct SyntaxCache {
    id: u64,
    revision: u64,
    name: String,
    text: Rope,
    checkpoints: BTreeMap<usize, (HighlightState, ParseState)>,
    visible: Option<(usize, usize, Vec<Vec<Style>>)>,
    pub pending: bool,
    #[cfg(test)]
    parsed: usize,
}

impl SyntaxCache {
    pub fn highlight(
        &mut self,
        buffer: &Buffer,
        syntaxes: &SyntaxSet,
        theme: &Theme,
        start: usize,
        end: usize,
    ) -> Vec<Vec<Style>> {
        let name = buffer.name();
        let syntax = name
            .rsplit_once('.')
            .and_then(|(_, extension)| syntaxes.find_syntax_by_extension(extension))
            .or_else(|| syntaxes.find_syntax_by_extension(&name))
            .or_else(|| syntaxes.find_syntax_by_first_line(&buffer.line(0)));
        let Some(syntax) = syntax else {
            self.pending = false;
            return vec![Vec::new(); end - start];
        };
        if self.id != buffer.id() || self.name != name {
            *self = Self::default();
            self.id = buffer.id();
            self.name = name;
            self.text = buffer.rope().clone();
            self.revision = buffer.revision();
            self.checkpoints
                .insert(0, HighlightLines::new(syntax, theme).state());
        } else if self.revision != buffer.revision() {
            let common = self
                .text
                .chars()
                .zip(buffer.rope().chars())
                .take_while(|(a, b)| a == b)
                .count();
            let line = self
                .text
                .char_to_line(common)
                .min(buffer.rope().char_to_line(common));
            self.checkpoints.retain(|at, _| *at <= line);
            self.text = buffer.rope().clone();
            self.revision = buffer.revision();
            self.visible = None;
        }
        if let Some((from, to, styles)) = &self.visible {
            if (*from, *to) == (start, end) {
                self.pending = false;
                return styles.clone();
            }
        }
        let (&from, state) = self
            .checkpoints
            .range(..=start)
            .next_back()
            .expect("initial syntax state");
        let mut highlighter = HighlightLines::from_state(theme, state.0.clone(), state.1.clone());
        let mut visible = Vec::with_capacity(end - start);
        let started = Instant::now();
        self.pending = false;
        for line in from..end {
            let text = buffer.line(line);
            // Pathological generated/minified lines should not monopolize the UI.
            let ranges = if text.len() <= 16_384 {
                highlighter
                    .highlight_line(&text, syntaxes)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            #[cfg(test)]
            {
                self.parsed += 1;
            }
            if line >= start {
                let mut styles = Vec::new();
                for (style, text) in ranges {
                    let mut converted = Style::default().fg(Color::Rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    ));
                    if style.font_style.contains(FontStyle::BOLD) {
                        converted = converted.add_modifier(Modifier::BOLD);
                    }
                    if style.font_style.contains(FontStyle::ITALIC) {
                        converted = converted.add_modifier(Modifier::ITALIC);
                    }
                    if style.font_style.contains(FontStyle::UNDERLINE) {
                        converted = converted.add_modifier(Modifier::UNDERLINED);
                    }
                    styles.extend(std::iter::repeat_n(converted, text.chars().count()));
                }
                visible.push(styles);
            }
            let pause = line < start && started.elapsed() >= Duration::from_millis(8);
            if (line + 1) % 128 == 0 || pause {
                let state = highlighter.state();
                highlighter = HighlightLines::from_state(theme, state.0.clone(), state.1.clone());
                self.checkpoints.insert(line + 1, state);
            }
            if pause {
                self.pending = true;
                return vec![Vec::new(); end - start];
            }
        }
        self.visible = Some((start, end, visible.clone()));
        visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reuses_state_and_invalidates_from_first_changed_line() {
        let syntax = SyntaxSet::load_defaults_newlines();
        let themes = syntect::highlighting::ThemeSet::load_defaults();
        let theme = &themes.themes["base16-eighties.dark"];
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.rs");
        std::fs::write(&path, "let value = 1;\n".repeat(1000)).unwrap();
        let mut buffer = Buffer::open(path).unwrap();
        let mut cache = SyntaxCache::default();
        while {
            cache.highlight(&buffer, &syntax, theme, 950, 980);
            cache.pending
        } {}
        let before = cache.parsed;
        cache.highlight(&buffer, &syntax, theme, 950, 980);
        assert_eq!(before, cache.parsed);
        buffer.set_cursor_line_col(960, 0, false);
        buffer.insert("/*");
        let styles = loop {
            let styles = cache.highlight(&buffer, &syntax, theme, 950, 980);
            if !cache.pending {
                break styles;
            }
        };
        assert!(cache.parsed - before < 128);
        let mut fresh = SyntaxCache::default();
        let expected = loop {
            let s = fresh.highlight(&buffer, &syntax, theme, 950, 980);
            if !fresh.pending {
                break s;
            }
        };
        assert_eq!(styles, expected);
    }
}
