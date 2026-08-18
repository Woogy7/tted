use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::Result;
use base64::Engine;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme},
    parsing::SyntaxSet,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::buffer::Buffer;
use crate::theme;

pub struct Editor {
    buffers: Vec<Buffer>,
    active: usize,
    top_line: usize,
    left_col: usize,
    body: Rect,
    tab_hits: Vec<(u16, u16)>,
    markdown_reading: Vec<bool>,
    clipboard: Option<String>,
    search_query: Option<String>,
    close_armed: Option<usize>,
    path_prompt: Option<String>,
    help_visible: bool,
    gutter_width: u16,
    sidebar_visible: bool,
    sidebar_area: Option<Rect>,
    sidebar_entries: Vec<PathBuf>,
    sidebar_hits: Vec<(u16, PathBuf)>,
    workspace_root: PathBuf,
    syntaxes: SyntaxSet,
    theme: Theme,
    message: String,
    quit_armed: bool,
}

impl Editor {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let mut buffers = Vec::new();
        let mut message = String::new();
        for path in paths {
            match Buffer::open(&path) {
                Ok(buffer) => buffers.push(buffer),
                Err(error) => message = format!("Could not open {}: {error}", path.display()),
            }
        }
        if buffers.is_empty() {
            buffers.push(Buffer::empty());
        }
        let markdown_reading = vec![false; buffers.len()];
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let themes = syntect::highlighting::ThemeSet::load_defaults();
        let theme = themes.themes["base16-eighties.dark"].clone();
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let sidebar_entries = workspace_files(&workspace_root);
        Self {
            buffers,
            active: 0,
            top_line: 0,
            left_col: 0,
            body: Rect::default(),
            tab_hits: Vec::new(),
            markdown_reading,
            clipboard: None,
            search_query: None,
            close_armed: None,
            path_prompt: None,
            help_visible: false,
            gutter_width: 0,
            sidebar_visible: false,
            sidebar_area: None,
            sidebar_entries,
            sidebar_hits: Vec::new(),
            workspace_root,
            syntaxes,
            theme,
            message,
            quit_armed: false,
        }
    }

    fn current(&self) -> &Buffer {
        &self.buffers[self.active]
    }
    fn current_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active]
    }

    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        terminal.draw(|frame| self.render(frame))?;
        loop {
            if event::poll(Duration::from_millis(250))? {
                let event = event::read()?;
                if self.handle_event(event)? {
                    return Ok(());
                }
                terminal.draw(|frame| self.render(frame))?;
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Key(key)
                if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
            {
                return self.key(key)
            }
            Event::Paste(text) => {
                if self.help_visible {
                    return Ok(false);
                } else if let Some(path) = &mut self.path_prompt {
                    path.push_str(text.trim_end_matches(['\r', '\n']));
                } else if let Some(query) = &mut self.search_query {
                    query.push_str(text.trim_end_matches(['\r', '\n']));
                } else {
                    self.current_mut().insert(&text.replace("\r\n", "\n"));
                    self.changed();
                }
            }
            Event::Mouse(_) if self.help_visible => {}
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if self
                        .sidebar_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
                {
                    if let Some((_, path)) = self
                        .sidebar_hits
                        .iter()
                        .find(|(row, _)| *row == mouse.row)
                        .cloned()
                    {
                        self.open_path(path);
                    }
                }
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row == self.body.y.saturating_sub(1) =>
                {
                    if let Some(tab) = self
                        .tab_hits
                        .iter()
                        .position(|(start, end)| mouse.column >= *start && mouse.column < *end)
                    {
                        self.active = tab;
                        self.reset_view();
                    }
                }
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
                    if !self.markdown_reading[self.active]
                        && self.body.contains((mouse.column, mouse.row).into()) =>
                {
                    let line = self.top_line + usize::from(mouse.row - self.body.y);
                    let col = self.left_col
                        + usize::from(mouse.column.saturating_sub(self.body.x + self.gutter_width));
                    let select = matches!(mouse.kind, MouseEventKind::Drag(_));
                    self.current_mut()
                        .set_cursor_line_screen_col(line, col, select);
                    self.ensure_visible();
                }
                MouseEventKind::ScrollUp => self.top_line = self.top_line.saturating_sub(3),
                MouseEventKind::ScrollDown => {
                    self.top_line =
                        (self.top_line + 3).min(self.current().len_lines().saturating_sub(1))
                }
                _ => {}
            },
            _ => {}
        }
        Ok(false)
    }

    fn key(&mut self, mut key: KeyEvent) -> Result<bool> {
        if let KeyCode::Char(control) = key.code {
            if let Some(letter) = control_letter(control) {
                key.code = KeyCode::Char(letter);
                key.modifiers.insert(KeyModifiers::CONTROL);
            }
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.help_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
                self.help_visible = false;
            }
            return Ok(false);
        }
        if key.code == KeyCode::F(1) {
            self.help_visible = true;
            return Ok(false);
        }
        if self.path_prompt.is_some() {
            return self.path_prompt_key(key);
        }
        if self.search_query.is_some() {
            return self.search_key(key);
        }
        if key.code == KeyCode::F(6)
            || (ctrl && shift && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M')))
        {
            if self
                .current()
                .path()
                .and_then(|path| path.extension())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
                })
            {
                self.markdown_reading[self.active] = !self.markdown_reading[self.active];
                self.top_line = 0;
                self.message = if self.markdown_reading[self.active] {
                    "Markdown reading view"
                } else {
                    "Markdown source view"
                }
                .into();
            } else {
                self.message = "Markdown reading view is available for .md files".into();
            }
            return Ok(false);
        }
        if alt && matches!(key.code, KeyCode::Left | KeyCode::Right) {
            if key.code == KeyCode::Right {
                self.active = (self.active + 1) % self.buffers.len();
            } else {
                self.active = (self.active + self.buffers.len() - 1) % self.buffers.len();
            }
            self.reset_view();
            return Ok(false);
        }
        if ctrl {
            match key.code {
                KeyCode::Char('q') => {
                    if self.buffers.iter().any(Buffer::is_dirty) && !self.quit_armed {
                        self.quit_armed = true;
                        self.message = "Unsaved changes — press Ctrl+Q again to quit".into();
                        return Ok(false);
                    }
                    return Ok(true);
                }
                KeyCode::Char('s') => {
                    if shift || self.current().path().is_none() {
                        self.path_prompt = Some(
                            self.current()
                                .path()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default(),
                        );
                        self.message.clear();
                    } else {
                        match self.current_mut().save() {
                            Ok(()) => self.message = "Saved".into(),
                            Err(error) => self.message = format!("Save failed: {error}"),
                        }
                    }
                }
                KeyCode::Char('f') => {
                    if self.markdown_reading[self.active] {
                        self.message = "Return to Markdown source before searching".into();
                    } else {
                        self.search_query = Some(String::new());
                        self.message.clear();
                    }
                }
                KeyCode::Char('b') => {
                    self.sidebar_visible = !self.sidebar_visible;
                    self.message = if self.sidebar_visible {
                        "File explorer opened"
                    } else {
                        "File explorer closed"
                    }
                    .into();
                }
                KeyCode::Char('w') => {
                    if self.current().is_dirty() && self.close_armed != Some(self.active) {
                        self.close_armed = Some(self.active);
                        self.message = "Unsaved changes — press Ctrl+W again to close tab".into();
                    } else {
                        self.close_current_tab();
                    }
                }
                KeyCode::Char('c') => {
                    if let Some(text) = self.current().selected_text() {
                        self.copy_to_terminal_clipboard(&text)?;
                        self.clipboard = Some(text);
                        self.message = "Copied selection".into();
                    } else {
                        self.message = "Nothing selected".into();
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(text) = self.current_mut().cut_selection() {
                        self.copy_to_terminal_clipboard(&text)?;
                        self.clipboard = Some(text);
                        self.message = "Cut selection".into();
                        self.changed();
                    } else {
                        self.message = "Nothing selected".into();
                    }
                }
                KeyCode::Char('v') => {
                    if let Some(text) = self.clipboard.clone() {
                        self.current_mut().insert(&text);
                        self.message = "Pasted TTED clipboard".into();
                        self.changed();
                    } else {
                        self.message =
                            "TTED clipboard is empty; use your terminal's paste shortcut".into();
                    }
                }
                KeyCode::Char('z') if shift => {
                    self.current_mut().redo();
                    self.changed();
                }
                KeyCode::Char('z') => {
                    self.current_mut().undo();
                    self.changed();
                }
                KeyCode::Char('y') => {
                    self.current_mut().redo();
                    self.changed();
                }
                KeyCode::Tab | KeyCode::PageDown if !self.buffers.is_empty() => {
                    self.active = (self.active + 1) % self.buffers.len();
                    self.reset_view();
                }
                KeyCode::BackTab | KeyCode::PageUp if !self.buffers.is_empty() => {
                    self.active = (self.active + self.buffers.len() - 1) % self.buffers.len();
                    self.reset_view();
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.markdown_reading[self.active] {
            let page = usize::from(self.body.height.max(1));
            match key.code {
                KeyCode::Up => self.top_line = self.top_line.saturating_sub(1),
                KeyCode::Down => self.top_line = self.top_line.saturating_add(1),
                KeyCode::PageUp => self.top_line = self.top_line.saturating_sub(page),
                KeyCode::PageDown => self.top_line = self.top_line.saturating_add(page),
                KeyCode::Home => self.top_line = 0,
                _ => {
                    self.message =
                        "Reading view is read-only; Ctrl+Shift+M returns to source".into()
                }
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Char(c) => {
                self.current_mut().insert(&c.to_string());
                self.changed();
            }
            KeyCode::Enter => {
                let prefix = self.current().current_line_prefix();
                let indent = prefix
                    .chars()
                    .take_while(|ch| ch.is_whitespace())
                    .collect::<String>();
                let opener = prefix
                    .trim_end()
                    .chars()
                    .last()
                    .is_some_and(|ch| matches!(ch, '{' | '[' | '('));
                let closer = self
                    .current()
                    .char_at_cursor()
                    .is_some_and(|ch| matches!(ch, '}' | ']' | ')'));
                let inner = if opener {
                    format!("{indent}    ")
                } else {
                    indent.clone()
                };
                if opener && closer {
                    let insertion = format!("\n{inner}\n{indent}");
                    let rewind = indent.chars().count() + 1;
                    self.current_mut().insert(&insertion);
                    self.current_mut()
                        .move_horizontal(-(rewind as isize), false);
                } else {
                    self.current_mut().insert(&format!("\n{inner}"));
                }
                self.changed();
            }
            KeyCode::Tab => {
                let col = self.current().cursor_screen_col();
                let spaces = 4 - (col % 4);
                self.current_mut().insert(&" ".repeat(spaces));
                self.changed();
            }
            KeyCode::BackTab => {
                self.current_mut().unindent_current_line(4);
                self.changed();
            }
            KeyCode::Backspace => {
                self.current_mut().smart_backspace(4);
                self.changed();
            }
            KeyCode::Delete => {
                self.current_mut().delete_forward();
                self.changed();
            }
            KeyCode::Left => self.current_mut().move_horizontal(-1, shift),
            KeyCode::Right => self.current_mut().move_horizontal(1, shift),
            KeyCode::Up => self.current_mut().move_vertical(-1, shift),
            KeyCode::Down => self.current_mut().move_vertical(1, shift),
            KeyCode::Home => self.current_mut().move_line_edge(false, shift),
            KeyCode::End => self.current_mut().move_line_edge(true, shift),
            KeyCode::PageUp => {
                let n = self.body.height.max(1) as isize;
                self.current_mut().move_vertical(-n, shift);
            }
            KeyCode::PageDown => {
                let n = self.body.height.max(1) as isize;
                self.current_mut().move_vertical(n, shift);
            }
            _ => {}
        }
        self.ensure_visible();
        self.quit_armed = false;
        self.close_armed = None;
        Ok(false)
    }

    fn search_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.search_query = None;
                self.message = "Search cancelled".into();
            }
            KeyCode::Enter => {
                let query = self.search_query.clone().unwrap_or_default();
                if self.current_mut().find_next(&query) {
                    self.message = format!("Found: {query}");
                    self.ensure_visible();
                } else {
                    self.message = format!("No match: {query}");
                }
                self.search_query = None;
            }
            KeyCode::Backspace => {
                self.search_query.as_mut().expect("search mode").pop();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query
                    .as_mut()
                    .expect("search mode")
                    .push(character);
            }
            _ => {}
        }
        Ok(false)
    }

    fn path_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.path_prompt = None;
                self.message = "Save As cancelled".into();
            }
            KeyCode::Enter => {
                let path = self.path_prompt.take().unwrap_or_default();
                if path.trim().is_empty() {
                    self.message = "Save As requires a path".into();
                } else {
                    match self.current_mut().save_as(path.trim()) {
                        Ok(()) => self.message = "Saved".into(),
                        Err(error) => self.message = format!("Save failed: {error}"),
                    }
                }
            }
            KeyCode::Backspace => {
                self.path_prompt.as_mut().expect("path prompt").pop();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.path_prompt
                    .as_mut()
                    .expect("path prompt")
                    .push(character);
            }
            _ => {}
        }
        Ok(false)
    }

    fn close_current_tab(&mut self) {
        self.buffers.remove(self.active);
        self.markdown_reading.remove(self.active);
        if self.buffers.is_empty() {
            self.buffers.push(Buffer::empty());
            self.markdown_reading.push(false);
        }
        self.active = self.active.min(self.buffers.len() - 1);
        self.close_armed = None;
        self.message = "Closed tab".into();
        self.reset_view();
    }

    fn open_path(&mut self, path: PathBuf) {
        if let Some(index) = self.buffers.iter().position(|buffer| {
            buffer
                .path()
                .is_some_and(|open| same_path(open, path.as_path()))
        }) {
            self.active = index;
            self.reset_view();
            return;
        }
        match Buffer::open(&path) {
            Ok(buffer) => {
                self.buffers.push(buffer);
                self.markdown_reading.push(false);
                self.active = self.buffers.len() - 1;
                self.message = format!("Opened {}", path.display());
                self.reset_view();
            }
            Err(error) => self.message = format!("Could not open {}: {error}", path.display()),
        }
    }

    fn copy_to_terminal_clipboard(&self, text: &str) -> Result<()> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(text);
        write!(io::stdout(), "\x1b]52;c;{encoded}\x07")?;
        io::stdout().flush()?;
        Ok(())
    }

    fn changed(&mut self) {
        self.quit_armed = false;
        self.ensure_visible();
    }
    fn reset_view(&mut self) {
        self.top_line = 0;
        self.left_col = 0;
        self.quit_armed = false;
    }
    fn ensure_visible(&mut self) {
        let (line, _) = self.current().cursor_line_col();
        let col = self.current().cursor_screen_col();
        let height = usize::from(self.body.height.max(1));
        let width = usize::from(self.body.width.saturating_sub(self.gutter_width).max(1));
        if line < self.top_line {
            self.top_line = line;
        } else if line >= self.top_line + height {
            self.top_line = line + 1 - height;
        }
        if col < self.left_col {
            self.left_col = col;
        } else if col >= self.left_col + width {
            self.left_col = col + 1 - width;
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
        self.sidebar_area = None;
        self.sidebar_hits.clear();
        if self.sidebar_visible && areas[1].width >= 40 {
            let sidebar_width = areas[1].width.saturating_div(3).clamp(18, 32);
            let columns =
                Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)])
                    .split(areas[1]);
            self.sidebar_area = Some(columns[0]);
            self.body = columns[1];
            self.render_sidebar(frame, columns[0]);
        } else {
            self.body = areas[1];
        }
        self.gutter_width = if self.markdown_reading[self.active] {
            0
        } else {
            ((self.top_line + usize::from(self.body.height)).min(self.current().len_lines()))
                .max(1)
                .to_string()
                .len() as u16
                + 2
        };
        self.ensure_visible();
        let mut tab_x = areas[0].x;
        self.tab_hits = self
            .buffers
            .iter()
            .map(|buffer| {
                let dirty = if buffer.is_dirty() { "●" } else { "" };
                let width =
                    UnicodeWidthStr::width(format!(" {}{} ", buffer.name(), dirty).as_str()) as u16;
                let hit = (tab_x, tab_x.saturating_add(width));
                tab_x = hit.1;
                hit
            })
            .collect();
        let tabs = self
            .buffers
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let dirty = if b.is_dirty() { "●" } else { "" };
                let label = format!(" {}{} ", b.name(), dirty);
                Span::styled(
                    label,
                    if i == self.active {
                        Style::default()
                            .bg(theme::MAUVE)
                            .fg(theme::BASE)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT0)
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(tabs)).style(Style::default().bg(theme::MANTLE)),
            areas[0],
        );

        if self.markdown_reading[self.active] {
            let rendered = crate::markdown::render(&self.current().text());
            self.top_line = self.top_line.min(rendered.len().saturating_sub(1));
            let visible = rendered
                .into_iter()
                .skip(self.top_line)
                .take(usize::from(self.body.height))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(visible).style(Style::default().bg(theme::BASE).fg(theme::TEXT)),
                self.body,
            );
            let status = format!(
                "{}   Markdown reading view   F1 help   Ctrl+Shift+M source",
                self.current().name()
            );
            frame.render_widget(
                Paragraph::new(status).style(Style::default().bg(theme::SURFACE0).fg(theme::TEXT)),
                areas[2],
            );
            if self.help_visible {
                self.render_help(frame);
            }
            return;
        }

        let selection = self.current().selection();
        let cursor = self.current().cursor();
        let syntax_styles = self.highlight_visible_lines(
            self.top_line,
            (self.top_line + usize::from(self.body.height)).min(self.current().len_lines()),
        );
        let mut lines = Vec::new();
        for (visible_index, line_idx) in (self.top_line
            ..(self.top_line + usize::from(self.body.height)).min(self.current().len_lines()))
            .enumerate()
        {
            let raw = self.current().line(line_idx);
            let line_start = self.current().line_start_char(line_idx);
            let mut spans = vec![Span::styled(
                format!(
                    "{:>width$} ",
                    line_idx + 1,
                    width = usize::from(self.gutter_width - 1)
                ),
                Style::default().fg(theme::OVERLAY0).bg(theme::MANTLE),
            )];
            let mut screen_col = 0usize;
            let mut char_offset = 0usize;
            let content = raw.trim_end_matches(['\n', '\r']);
            for grapheme in content.graphemes(true) {
                let char_count = grapheme.chars().count();
                let width = UnicodeWidthStr::width(grapheme);
                if screen_col + width > self.left_col {
                    let grapheme_start = line_start + char_offset;
                    let grapheme_end = grapheme_start + char_count;
                    let selected =
                        selection.is_some_and(|(a, b)| grapheme_start < b && grapheme_end > a);
                    let syntax_style = syntax_styles
                        .get(visible_index)
                        .and_then(|styles| styles.get(char_offset))
                        .copied()
                        .unwrap_or_default();
                    spans.push(Span::styled(
                        grapheme.to_owned(),
                        if selected {
                            syntax_style.bg(theme::SURFACE1).fg(theme::TEXT)
                        } else {
                            syntax_style
                        },
                    ));
                }
                screen_col += width;
                char_offset += char_count;
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(Block::default().borders(Borders::NONE)),
            self.body,
        );

        let (line, char_col) = self.current().cursor_line_col();
        let screen_col = self.current().cursor_screen_col();
        let path = self
            .current()
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Untitled".into());
        let left = if let Some(path) = &self.path_prompt {
            format!("Save As: {path}_   Enter save  Esc cancel")
        } else if let Some(query) = &self.search_query {
            format!("Find: {query}_   Enter search  Esc cancel")
        } else if self.message.is_empty() {
            path
        } else {
            self.message.clone()
        };
        let status = format!(
            "{left}   Ln {}, Col {}   F1 help  Ctrl+S save  Ctrl+Q quit",
            line + 1,
            char_col + 1
        );
        frame.render_widget(
            Paragraph::new(status).style(Style::default().bg(theme::SURFACE0).fg(theme::TEXT)),
            areas[2],
        );
        if !self.help_visible
            && line >= self.top_line
            && line < self.top_line + usize::from(self.body.height)
        {
            let x = self
                .body
                .x
                .saturating_add(self.gutter_width)
                .saturating_add(
                    screen_col.saturating_sub(self.left_col).min(usize::from(
                        self.body
                            .width
                            .saturating_sub(self.gutter_width.saturating_add(1)),
                    )) as u16,
                );
            let y = self.body.y + (line - self.top_line) as u16;
            frame.set_cursor_position((x, y));
        }
        let _ = cursor;
        if self.help_visible {
            self.render_help(frame);
        }
    }

    fn render_help(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.min(72);
        let height = area.height.min(22);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let help = [
            "Navigation",
            "  Arrows / Home / End / Page Up/Down   Move",
            "  Shift + navigation                    Select",
            "  Alt+Left / Alt+Right                  Switch tabs",
            "",
            "Editing",
            "  Ctrl+C / Ctrl+X / Ctrl+V              Copy / cut / paste",
            "  Ctrl+Z / Ctrl+Y                       Undo / redo",
            "  Tab / Shift+Tab                       Indent / unindent",
            "",
            "Files and views",
            "  Ctrl+S / Ctrl+Shift+S                 Save / Save As",
            "  Ctrl+F                                Find",
            "  Ctrl+W                                Close tab",
            "  Ctrl+B                                Toggle file explorer",
            "  Ctrl+Shift+M / F6                     Markdown reading view",
            "  Ctrl+Q                                Quit",
            "",
            "  F1 or Esc closes this help",
        ];
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(help.join("\n"))
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(
                    Block::bordered()
                        .title(" TTED keybindings ")
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            popup,
        );
    }

    fn render_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        let inner_height = area.height.saturating_sub(2) as usize;
        let mut lines = Vec::with_capacity(inner_height);
        for (index, path) in self.sidebar_entries.iter().take(inner_height).enumerate() {
            let relative = path.strip_prefix(&self.workspace_root).unwrap_or(path);
            let label = format!(" {}", relative.display());
            let active = self
                .current()
                .path()
                .is_some_and(|open| same_path(open, path.as_path()));
            lines.push(Line::styled(
                label,
                if active {
                    Style::default().bg(theme::SURFACE1).fg(theme::MAUVE)
                } else {
                    Style::default().fg(theme::SUBTEXT0)
                },
            ));
            self.sidebar_hits
                .push((area.y + 1 + index as u16, path.clone()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::MANTLE))
                .block(
                    Block::bordered()
                        .title(" Files ")
                        .border_style(Style::default().fg(theme::SURFACE1)),
                ),
            area,
        );
    }

    fn highlight_visible_lines(&self, start: usize, end: usize) -> Vec<Vec<Style>> {
        let Some(path) = self.current().path() else {
            return vec![Vec::new(); end - start];
        };
        let Ok(Some(syntax)) = self.syntaxes.find_syntax_for_file(path) else {
            return vec![Vec::new(); end - start];
        };
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut visible = Vec::with_capacity(end - start);
        for line_index in 0..end {
            let line = self.current().line(line_index);
            let ranges = highlighter
                .highlight_line(&line, &self.syntaxes)
                .unwrap_or_default();
            if line_index < start {
                continue;
            }
            let mut styles = Vec::with_capacity(line.chars().count());
            for (style, text) in ranges {
                let foreground =
                    Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                let mut terminal_style = Style::default().fg(foreground);
                if style.font_style.contains(FontStyle::BOLD) {
                    terminal_style = terminal_style.add_modifier(Modifier::BOLD);
                }
                if style.font_style.contains(FontStyle::ITALIC) {
                    terminal_style = terminal_style.add_modifier(Modifier::ITALIC);
                }
                if style.font_style.contains(FontStyle::UNDERLINE) {
                    terminal_style = terminal_style.add_modifier(Modifier::UNDERLINED);
                }
                styles.extend(std::iter::repeat_n(terminal_style, text.chars().count()));
            }
            visible.push(styles);
        }
        visible
    }
}

fn control_letter(character: char) -> Option<char> {
    let value = character as u32;
    (1..=26)
        .contains(&value)
        .then(|| char::from_u32(value + 96).expect("ASCII control mapping"))
}

fn workspace_files(root: &std::path::Path) -> Vec<PathBuf> {
    fn visit(directory: &std::path::Path, depth: usize, files: &mut Vec<PathBuf>) {
        if depth > 3 || files.len() >= 250 {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if files.len() >= 250 {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                visit(&path, depth + 1, files);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    visit(root, 0, &mut files);
    files
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod input_tests {
    use std::fs;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{control_letter, workspace_files, Editor};

    #[test]
    fn maps_raw_control_characters() {
        assert_eq!(control_letter('\u{3}'), Some('c'));
        assert_eq!(control_letter('\u{11}'), Some('q'));
        assert_eq!(control_letter('q'), None);
    }

    #[test]
    fn explorer_skips_hidden_and_build_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        fs::create_dir(root.path().join(".hidden")).unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.path().join("target/output"), "ignored").unwrap();
        fs::write(root.path().join(".hidden/secret"), "ignored").unwrap();
        let files = workspace_files(root.path());
        assert_eq!(files, vec![root.path().join("src/main.rs")]);
    }

    #[test]
    fn ctrl_q_quits_from_markdown_reading_view() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.md");
        fs::write(&path, "# Notes").unwrap();
        let mut editor = Editor::new(vec![path]);
        editor.markdown_reading[0] = true;
        let quit = editor
            .key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
            .unwrap();
        assert!(quit);
    }
}
