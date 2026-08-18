use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
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

use crate::buffer::{Buffer, ExternalChange};
use crate::command::{Command, CommandPalette};
use crate::explorer::{Explorer, ExplorerAction};
use crate::quick_open::QuickOpen;
use crate::theme;

#[derive(Clone, Copy)]
struct ExternalPrompt {
    buffer: usize,
    change: ExternalChange,
}

#[derive(Clone, Copy)]
enum ExplorerPromptKind {
    NewFile,
    NewDirectory,
    Rename,
}

struct ExplorerPrompt {
    kind: ExplorerPromptKind,
    input: String,
    base: PathBuf,
    source: Option<PathBuf>,
}

struct SearchState {
    query: String,
    replacement: String,
    editing_replacement: bool,
    case_sensitive: bool,
}

pub struct Editor {
    buffers: Vec<Buffer>,
    active: usize,
    top_line: usize,
    left_col: usize,
    body: Rect,
    tab_hits: Vec<(u16, u16, usize)>,
    tab_close_hits: Vec<(u16, u16, usize)>,
    tab_start: usize,
    markdown_reading: Vec<bool>,
    clipboard: Option<String>,
    search: Option<SearchState>,
    quick_open: Option<QuickOpen>,
    command_palette: Option<CommandPalette>,
    close_armed: Option<usize>,
    path_prompt: Option<String>,
    help_visible: bool,
    gutter_width: u16,
    sidebar_visible: bool,
    focus_mode: bool,
    sidebar_before_focus: bool,
    sidebar_area: Option<Rect>,
    explorer: Explorer,
    explorer_prompt: Option<ExplorerPrompt>,
    delete_confirm: Option<PathBuf>,
    explorer_context_visible: bool,
    external_prompt: Option<ExternalPrompt>,
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
        Self {
            buffers,
            active: 0,
            top_line: 0,
            left_col: 0,
            body: Rect::default(),
            tab_hits: Vec::new(),
            tab_close_hits: Vec::new(),
            tab_start: 0,
            markdown_reading,
            clipboard: None,
            search: None,
            quick_open: None,
            command_palette: None,
            close_armed: None,
            path_prompt: None,
            help_visible: false,
            gutter_width: 0,
            sidebar_visible: false,
            focus_mode: false,
            sidebar_before_focus: false,
            sidebar_area: None,
            explorer: Explorer::new(workspace_root),
            explorer_prompt: None,
            delete_confirm: None,
            explorer_context_visible: false,
            external_prompt: None,
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
        let mut last_disk_check = Instant::now();
        loop {
            let mut redraw = false;
            if event::poll(Duration::from_millis(100))? {
                let event = event::read()?;
                if self.handle_event(event)? {
                    return Ok(());
                }
                redraw = true;
            }
            if last_disk_check.elapsed() >= Duration::from_millis(500) {
                redraw |= self.check_external_files();
                last_disk_check = Instant::now();
            }
            if redraw {
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
                if self.help_visible
                    || self.external_prompt.is_some()
                    || self.delete_confirm.is_some()
                    || self.close_armed.is_some()
                {
                    return Ok(false);
                } else if let Some(path) = &mut self.path_prompt {
                    path.push_str(text.trim_end_matches(['\r', '\n']));
                } else if let Some(search) = &mut self.search {
                    let target = if search.editing_replacement {
                        &mut search.replacement
                    } else {
                        &mut search.query
                    };
                    target.push_str(text.trim_end_matches(['\r', '\n']));
                    self.refresh_search_selection();
                } else if let Some(picker) = &mut self.quick_open {
                    picker.push_str(text.trim_end_matches(['\r', '\n']));
                } else if let Some(palette) = &mut self.command_palette {
                    palette.push_str(text.trim_end_matches(['\r', '\n']));
                } else if let Some(prompt) = &mut self.explorer_prompt {
                    prompt.input.push_str(text.trim_end_matches(['\r', '\n']));
                } else {
                    self.current_mut().insert(&text.replace("\r\n", "\n"));
                    self.changed();
                }
            }
            Event::Mouse(_)
                if self.help_visible
                    || self.external_prompt.is_some()
                    || self.explorer_prompt.is_some()
                    || self.delete_confirm.is_some()
                    || self.close_armed.is_some()
                    || self.quick_open.is_some()
                    || self.search.is_some()
                    || self.command_palette.is_some()
                    || self.explorer_context_visible => {}
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Right)
                    if self
                        .sidebar_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
                {
                    let area = self.sidebar_area.expect("sidebar area checked above");
                    let height = usize::from(area.height.saturating_sub(2).max(1));
                    let visible = usize::from(mouse.row.saturating_sub(area.y + 1));
                    if mouse.row > area.y
                        && mouse.row < area.y + area.height.saturating_sub(1)
                        && self.explorer.scroll() + visible < self.explorer.rows().len()
                    {
                        self.explorer.select_visible(visible, height);
                        self.explorer.set_focused(true);
                        self.explorer_context_visible = true;
                    }
                }
                MouseEventKind::ScrollUp
                    if self
                        .sidebar_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
                {
                    let height = self
                        .sidebar_area
                        .map_or(1, |area| usize::from(area.height.saturating_sub(2).max(1)));
                    self.explorer.scroll_by(-3, height);
                }
                MouseEventKind::ScrollDown
                    if self
                        .sidebar_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
                {
                    let height = self
                        .sidebar_area
                        .map_or(1, |area| usize::from(area.height.saturating_sub(2).max(1)));
                    self.explorer.scroll_by(3, height);
                }
                MouseEventKind::Down(MouseButton::Left)
                    if self
                        .sidebar_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
                {
                    let area = self.sidebar_area.expect("sidebar area checked above");
                    let height = usize::from(area.height.saturating_sub(2).max(1));
                    let visible = usize::from(mouse.row.saturating_sub(area.y + 1));
                    self.explorer.set_focused(true);
                    if mouse.row > area.y
                        && mouse.row < area.y + area.height.saturating_sub(1)
                        && self.explorer.scroll() + visible < self.explorer.rows().len()
                    {
                        self.explorer.select_visible(visible, height);
                        if let ExplorerAction::Open(path) = self.explorer.activate_selected() {
                            self.open_path(path);
                            self.explorer.set_focused(false);
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row == self.body.y.saturating_sub(1) =>
                {
                    self.explorer.set_focused(false);
                    if let Some((_, _, tab)) = self
                        .tab_close_hits
                        .iter()
                        .find(|(start, end, _)| mouse.column >= *start && mouse.column < *end)
                        .copied()
                    {
                        self.request_close_tab(tab);
                    } else if let Some((_, _, tab)) = self
                        .tab_hits
                        .iter()
                        .find(|(start, end, _)| mouse.column >= *start && mouse.column < *end)
                        .copied()
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
                    self.explorer.set_focused(false);
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
                    let max_top = if self.markdown_reading[self.active] {
                        self.markdown_max_top()
                    } else {
                        self.current().len_lines().saturating_sub(1)
                    };
                    self.top_line = (self.top_line + 3).min(max_top)
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
            return self.execute_command(Command::ShowHelp);
        }
        if self.external_prompt.is_some() {
            return self.external_prompt_key(key);
        }
        if self.close_armed.is_some() {
            return self.close_confirm_key(key);
        }
        if self.delete_confirm.is_some() {
            return self.delete_confirm_key(key);
        }
        if self.explorer_context_visible {
            return self.explorer_context_key(key);
        }
        if self.explorer_prompt.is_some() {
            return self.explorer_prompt_key(key);
        }
        if self.path_prompt.is_some() {
            return self.path_prompt_key(key);
        }
        if self.search.is_some() {
            return self.search_key(key);
        }
        if self.quick_open.is_some() {
            return self.quick_open_key(key);
        }
        if self.command_palette.is_some() {
            return self.command_palette_key(key);
        }
        if ctrl && shift && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')) {
            self.command_palette = Some(CommandPalette::new());
            return Ok(false);
        }
        if key.code == KeyCode::F(11) {
            return self.execute_command(Command::FocusMode);
        }
        if key.code == KeyCode::F(6)
            || (ctrl && shift && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M')))
        {
            return self.execute_command(Command::ToggleMarkdownReader);
        }
        if alt && matches!(key.code, KeyCode::Left | KeyCode::Right) {
            return self.execute_command(if key.code == KeyCode::Right {
                Command::NextTab
            } else {
                Command::PreviousTab
            });
        }
        if ctrl && key.code == KeyCode::Char('n') {
            return self.execute_command(Command::NewFile);
        }
        if ctrl && key.code == KeyCode::Char('p') {
            return self.execute_command(Command::QuickOpen);
        }
        if ctrl && key.code == KeyCode::Char('e') {
            return self.execute_command(Command::ToggleExplorer);
        }
        if self.sidebar_visible && self.explorer.focused() && !ctrl && !alt {
            let height = self
                .sidebar_area
                .map_or(1, |area| usize::from(area.height.saturating_sub(2).max(1)));
            match key.code {
                KeyCode::Esc | KeyCode::Tab => self.explorer.set_focused(false),
                KeyCode::Up => self.explorer.move_selection(-1, height),
                KeyCode::Down => self.explorer.move_selection(1, height),
                KeyCode::PageUp => self.explorer.move_selection(-(height as isize), height),
                KeyCode::PageDown => self.explorer.move_selection(height as isize, height),
                KeyCode::Home => self.explorer.select_first(),
                KeyCode::End => self.explorer.select_last(height),
                KeyCode::Left => self.explorer.collapse_or_parent(height),
                KeyCode::Right => self.explorer.expand_selected(),
                KeyCode::Enter => {
                    if let ExplorerAction::Open(path) = self.explorer.activate_selected() {
                        self.open_path(path);
                        self.explorer.set_focused(false);
                    }
                }
                KeyCode::Char('n' | 'N') if shift => {
                    self.start_explorer_prompt(ExplorerPromptKind::NewDirectory)
                }
                KeyCode::Char('n') => self.start_explorer_prompt(ExplorerPromptKind::NewFile),
                KeyCode::Char('r') => self.start_explorer_prompt(ExplorerPromptKind::Rename),
                KeyCode::Char('d') => {
                    if let Some(path) = self.explorer.selected_path().map(PathBuf::from) {
                        if self.path_is_open(&path) {
                            self.message = "Close the file before deleting it".into();
                        } else {
                            self.delete_confirm = Some(path);
                        }
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        if ctrl {
            match key.code {
                KeyCode::Char('q') => {
                    return self.execute_command(Command::Quit);
                }
                KeyCode::Char('s') => {
                    return self.execute_command(if shift {
                        Command::SaveAs
                    } else {
                        Command::Save
                    });
                }
                KeyCode::Char('f') => {
                    return self.execute_command(Command::FindReplace);
                }
                KeyCode::Char('w') => {
                    return self.execute_command(Command::CloseFile);
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
                    return self.execute_command(Command::NextTab);
                }
                KeyCode::BackTab | KeyCode::PageUp if !self.buffers.is_empty() => {
                    return self.execute_command(Command::PreviousTab);
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.markdown_reading[self.active] {
            let page = usize::from(self.body.height.max(1));
            let max_top = self.markdown_max_top();
            match key.code {
                KeyCode::Up => self.top_line = self.top_line.saturating_sub(1),
                KeyCode::Down => self.top_line = self.top_line.saturating_add(1).min(max_top),
                KeyCode::PageUp => self.top_line = self.top_line.saturating_sub(page),
                KeyCode::PageDown => {
                    self.top_line = self.top_line.saturating_add(page).min(max_top)
                }
                KeyCode::Home => self.top_line = 0,
                KeyCode::End => self.top_line = max_top,
                _ => {
                    self.message =
                        "Reading view is read-only; Ctrl+Shift+M returns to source".into()
                }
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Char(c) => {
                if matches!(c, '}' | ']' | ')')
                    && self
                        .current()
                        .current_line_prefix()
                        .chars()
                        .all(char::is_whitespace)
                {
                    self.current_mut().unindent_current_line(4);
                }
                self.current_mut().insert_typed(&c.to_string());
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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => {
                self.search = None;
                self.message = "Search cancelled".into();
            }
            KeyCode::Enter | KeyCode::F(3) => {
                let search = self.search.as_ref().expect("search mode");
                let query = search.query.clone();
                let case_sensitive = search.case_sensitive;
                if let Some((current, total)) =
                    self.current_mut()
                        .find_search(&query, case_sensitive, shift)
                {
                    self.message = format!("Match {current} of {total}");
                    self.ensure_visible();
                } else {
                    self.message = format!("No match: {query}");
                }
            }
            KeyCode::Backspace => {
                let search = self.search.as_mut().expect("search mode");
                if search.editing_replacement {
                    search.replacement.pop();
                } else {
                    search.query.pop();
                }
                self.refresh_search_selection();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let search = self.search.as_mut().expect("search mode");
                search.editing_replacement = !search.editing_replacement;
            }
            KeyCode::Char('c' | 'C') if alt => {
                let search = self.search.as_mut().expect("search mode");
                search.case_sensitive = !search.case_sensitive;
                self.refresh_search_selection();
            }
            KeyCode::Char('r' | 'R') if ctrl => {
                let search = self.search.as_ref().expect("search mode");
                let query = search.query.clone();
                let replacement = search.replacement.clone();
                let case_sensitive = search.case_sensitive;
                if shift {
                    let count =
                        self.current_mut()
                            .replace_all_search(&query, &replacement, case_sensitive);
                    self.message = format!("Replaced {count} matches");
                } else if self.current_mut().replace_search_selection(
                    &query,
                    &replacement,
                    case_sensitive,
                ) {
                    self.current_mut()
                        .find_search(&query, case_sensitive, false);
                    self.message = "Replaced current match".into();
                } else {
                    self.message = "Select a match with Enter before replacing".into();
                }
                self.changed();
            }
            KeyCode::Char(character) if !ctrl && !alt => {
                let search = self.search.as_mut().expect("search mode");
                if search.editing_replacement {
                    search.replacement.push(character);
                } else {
                    search.query.push(character);
                }
                self.refresh_search_selection();
            }
            _ => {}
        }
        Ok(false)
    }

    fn refresh_search_selection(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        let query = search.query.clone();
        let case_sensitive = search.case_sensitive;
        if self.current_mut().refresh_search(&query, case_sensitive) {
            self.ensure_visible();
        }
    }

    fn quick_open_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.quick_open = None,
            KeyCode::Backspace => self.quick_open.as_mut().expect("quick open").pop(),
            KeyCode::Up => self
                .quick_open
                .as_mut()
                .expect("quick open")
                .move_selection(-1),
            KeyCode::Down => self
                .quick_open
                .as_mut()
                .expect("quick open")
                .move_selection(1),
            KeyCode::PageUp => self
                .quick_open
                .as_mut()
                .expect("quick open")
                .move_selection(-8),
            KeyCode::PageDown => self
                .quick_open
                .as_mut()
                .expect("quick open")
                .move_selection(8),
            KeyCode::Enter => {
                let path = self.quick_open.as_ref().and_then(QuickOpen::selected_path);
                self.quick_open = None;
                if let Some(path) = path {
                    self.open_path(path);
                    self.explorer.set_focused(false);
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => self
                .quick_open
                .as_mut()
                .expect("quick open")
                .push(character),
            _ => {}
        }
        Ok(false)
    }

    fn command_palette_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.command_palette = None,
            KeyCode::Backspace => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .pop(),
            KeyCode::Up => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .move_selection(-1),
            KeyCode::Down => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .move_selection(1),
            KeyCode::PageUp => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .move_selection(-8),
            KeyCode::PageDown => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .move_selection(8),
            KeyCode::Enter => {
                let command = self
                    .command_palette
                    .as_ref()
                    .and_then(CommandPalette::selected_command);
                self.command_palette = None;
                if let Some(command) = command {
                    return self.execute_command(command);
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => self
                .command_palette
                .as_mut()
                .expect("command palette")
                .push(character),
            _ => {}
        }
        Ok(false)
    }

    fn execute_command(&mut self, command: Command) -> Result<bool> {
        match command {
            Command::NewFile => {
                self.explorer_prompt = Some(ExplorerPrompt {
                    kind: ExplorerPromptKind::NewFile,
                    input: String::new(),
                    base: self.explorer.root().to_path_buf(),
                    source: None,
                });
            }
            Command::Save | Command::SaveAs => {
                if command == Command::SaveAs || self.current().path().is_none() {
                    self.path_prompt = Some(
                        self.current()
                            .path()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default(),
                    );
                    self.message.clear();
                } else {
                    match self.current_mut().save() {
                        Ok(()) => {
                            self.explorer.refresh();
                            self.message = "Saved".into();
                        }
                        Err(error) => self.message = format!("Save failed: {error}"),
                    }
                }
            }
            Command::CloseFile => self.request_close_tab(self.active),
            Command::QuickOpen => {
                self.quick_open = Some(QuickOpen::new(self.explorer.root().to_path_buf()))
            }
            Command::ToggleExplorer => {
                if self.focus_mode {
                    self.message = "Exit Focus Mode with F11 before opening the explorer".into();
                } else {
                    self.sidebar_visible = !self.sidebar_visible;
                    self.explorer.set_focused(self.sidebar_visible);
                    if self.sidebar_visible {
                        self.explorer.refresh();
                    }
                    self.message = if self.sidebar_visible {
                        "File explorer opened and focused"
                    } else {
                        "File explorer closed"
                    }
                    .into();
                }
            }
            Command::FocusMode => self.toggle_focus_mode(),
            Command::FindReplace => {
                if self.markdown_reading[self.active] {
                    self.message = "Return to Markdown source before searching".into();
                } else {
                    self.search = Some(SearchState {
                        query: String::new(),
                        replacement: String::new(),
                        editing_replacement: false,
                        case_sensitive: false,
                    });
                    self.message.clear();
                }
            }
            Command::NextTab | Command::PreviousTab => {
                if !self.buffers.is_empty() {
                    if command == Command::NextTab {
                        self.active = (self.active + 1) % self.buffers.len();
                    } else {
                        self.active = (self.active + self.buffers.len() - 1) % self.buffers.len();
                    }
                    self.reset_view();
                }
            }
            Command::ToggleMarkdownReader => {
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
            }
            Command::ShowHelp => self.help_visible = true,
            Command::Quit => {
                if self.buffers.iter().any(Buffer::is_dirty) && !self.quit_armed {
                    self.quit_armed = true;
                    self.message = "Unsaved changes — press Ctrl+Q again to quit".into();
                } else {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn start_explorer_prompt(&mut self, kind: ExplorerPromptKind) {
        let source = matches!(kind, ExplorerPromptKind::Rename)
            .then(|| self.explorer.selected_path().map(PathBuf::from))
            .flatten();
        if matches!(kind, ExplorerPromptKind::Rename)
            && source.as_ref().is_some_and(|path| self.path_is_open(path))
        {
            self.message = "Close files before renaming them or their parent folder".into();
            return;
        }
        let base = source
            .as_deref()
            .and_then(Path::parent)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.explorer.operation_directory().to_path_buf());
        let input = source
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(String::new, |name| name.to_string_lossy().into());
        self.explorer_prompt = Some(ExplorerPrompt {
            kind,
            input,
            base,
            source,
        });
    }

    fn explorer_context_key(&mut self, key: KeyEvent) -> Result<bool> {
        self.explorer_context_visible = false;
        match key.code {
            KeyCode::Char('n' | 'N') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.start_explorer_prompt(ExplorerPromptKind::NewDirectory)
            }
            KeyCode::Char('n') => self.start_explorer_prompt(ExplorerPromptKind::NewFile),
            KeyCode::Char('r' | 'R') => self.start_explorer_prompt(ExplorerPromptKind::Rename),
            KeyCode::Char('d' | 'D') => {
                if let Some(path) = self.explorer.selected_path().map(PathBuf::from) {
                    if self.path_is_open(&path) {
                        self.message = "Close the file before deleting it".into();
                    } else {
                        self.delete_confirm = Some(path);
                    }
                }
            }
            KeyCode::Esc => {}
            _ => self.explorer_context_visible = true,
        }
        Ok(false)
    }

    fn explorer_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.explorer_prompt = None;
                self.message = "File operation cancelled".into();
            }
            KeyCode::Backspace => {
                self.explorer_prompt
                    .as_mut()
                    .expect("explorer prompt")
                    .input
                    .pop();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.explorer_prompt
                    .as_mut()
                    .expect("explorer prompt")
                    .input
                    .push(character);
            }
            KeyCode::Enter => {
                let prompt = self.explorer_prompt.take().expect("explorer prompt");
                if !valid_entry_name(&prompt.input) {
                    self.message = "Use a single non-empty file or directory name".into();
                    self.explorer_prompt = Some(prompt);
                    return Ok(false);
                }
                let destination = prompt.base.join(&prompt.input);
                if matches!(prompt.kind, ExplorerPromptKind::Rename)
                    && destination.exists()
                    && prompt
                        .source
                        .as_deref()
                        .is_none_or(|source| !same_path(source, &destination))
                {
                    self.message = "Rename destination already exists".into();
                    self.explorer_prompt = Some(prompt);
                    return Ok(false);
                }
                let result = match prompt.kind {
                    ExplorerPromptKind::NewFile => fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&destination)
                        .map(|_| ()),
                    ExplorerPromptKind::NewDirectory => fs::create_dir(&destination),
                    ExplorerPromptKind::Rename => fs::rename(
                        prompt.source.as_ref().expect("rename prompt has source"),
                        &destination,
                    ),
                };
                match result {
                    Ok(()) => {
                        self.explorer.refresh();
                        self.message = format!("Updated {}", destination.display());
                        if matches!(prompt.kind, ExplorerPromptKind::NewFile) {
                            self.open_path(destination);
                            self.explorer.set_focused(false);
                        }
                    }
                    Err(error) => self.message = format!("File operation failed: {error}"),
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn delete_confirm_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                let path = self.delete_confirm.take().expect("delete confirmation");
                let result = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                match result {
                    Ok(()) => {
                        self.explorer.refresh();
                        self.message = format!("Deleted {}", path.display());
                    }
                    Err(error) => self.message = format!("Delete failed: {error}"),
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.delete_confirm = None;
                self.message = "Delete cancelled".into();
            }
            _ => {}
        }
        Ok(false)
    }

    fn path_is_open(&self, path: &Path) -> bool {
        self.buffers.iter().any(|buffer| {
            buffer.path().is_some_and(|open| {
                same_path(open, path) || (path.is_dir() && open.starts_with(path))
            })
        })
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
                        Ok(()) => {
                            self.explorer.refresh();
                            self.message = "Saved".into();
                        }
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

    fn external_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        let Some(prompt) = self.external_prompt else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Char('r' | 'R') => match self.buffers[prompt.buffer].reload_from_disk() {
                Ok(()) => {
                    self.external_prompt = None;
                    self.message = "Reloaded file from disk".into();
                    self.ensure_visible();
                }
                Err(error) => self.message = format!("Reload failed: {error}; K keeps the buffer"),
            },
            KeyCode::Char('k' | 'K') => {
                self.buffers[prompt.buffer].keep_after_external_change();
                self.external_prompt = None;
                self.message = if prompt.change == ExternalChange::Deleted {
                    "Keeping deleted file in the editor; Save will recreate it"
                } else {
                    "Keeping editor version; Save will overwrite the disk version"
                }
                .into();
            }
            _ => {}
        }
        Ok(false)
    }

    fn check_external_files(&mut self) -> bool {
        if self.external_prompt.is_some() {
            return false;
        }
        for index in 0..self.buffers.len() {
            match self.buffers[index].check_external_change() {
                Ok(ExternalChange::None) => {}
                Ok(ExternalChange::Modified) if !self.buffers[index].is_dirty() => {
                    match self.buffers[index].reload_from_disk() {
                        Ok(()) => {
                            self.message = format!(
                                "Reloaded {} after an external change",
                                self.buffers[index].name()
                            )
                        }
                        Err(error) => {
                            self.message =
                                format!("Could not reload {}: {error}", self.buffers[index].name())
                        }
                    }
                    return true;
                }
                Ok(change) => {
                    self.active = index;
                    self.reset_view();
                    self.external_prompt = Some(ExternalPrompt {
                        buffer: index,
                        change,
                    });
                    return true;
                }
                Err(error) => {
                    self.message = format!(
                        "Could not check {} for changes: {error}",
                        self.buffers[index].name()
                    );
                    return true;
                }
            }
        }
        false
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

    fn request_close_tab(&mut self, tab: usize) {
        if tab >= self.buffers.len() {
            return;
        }
        self.active = tab;
        if self.current().is_dirty() {
            self.close_armed = Some(tab);
        } else {
            self.close_current_tab();
        }
    }

    fn close_confirm_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                if let Some(tab) = self.close_armed {
                    self.active = tab.min(self.buffers.len().saturating_sub(1));
                    self.close_current_tab();
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.close_armed = None;
                self.message = "Close cancelled".into();
            }
            _ => {}
        }
        Ok(false)
    }

    fn toggle_focus_mode(&mut self) {
        if self.focus_mode {
            self.focus_mode = false;
            self.sidebar_visible = self.sidebar_before_focus;
            self.message = "Focus Mode off".into();
        } else {
            self.sidebar_before_focus = self.sidebar_visible;
            self.sidebar_visible = false;
            self.explorer.set_focused(false);
            self.focus_mode = true;
            self.message = "Focus Mode on — F11 restores the workspace".into();
        }
    }

    fn ensure_active_tab_visible(&mut self, available_width: usize) {
        self.tab_start = self.tab_start.min(self.active);
        loop {
            let leading = usize::from(self.tab_start > 0) * 2;
            let width = self.buffers[self.tab_start..=self.active]
                .iter()
                .map(|buffer| {
                    let dirty = if buffer.is_dirty() { "●" } else { "" };
                    UnicodeWidthStr::width(format!(" {}{} × ", buffer.name(), dirty).as_str())
                })
                .sum::<usize>()
                + leading;
            if width <= available_width || self.tab_start == self.active {
                break;
            }
            self.tab_start += 1;
        }
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

    fn markdown_max_top(&self) -> usize {
        crate::markdown::render(&self.current().text())
            .len()
            .saturating_sub(usize::from(self.body.height.max(1)))
    }

    fn render(&mut self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
        self.sidebar_area = None;
        if self.focus_mode {
            self.body = frame.area();
        } else if self.sidebar_visible && areas[1].width >= 40 {
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
        if !self.markdown_reading[self.active] {
            self.ensure_visible();
        }
        self.tab_hits.clear();
        self.tab_close_hits.clear();
        if !self.focus_mode {
            self.ensure_active_tab_visible(usize::from(areas[0].width));
            let mut tab_x = areas[0].x;
            let mut tabs = Vec::new();
            if self.tab_start > 0 {
                tabs.push(Span::styled("‹ ", Style::default().fg(theme::OVERLAY0)));
                tab_x += 2;
            }
            for (index, buffer) in self.buffers.iter().enumerate().skip(self.tab_start) {
                let dirty = if buffer.is_dirty() { "●" } else { "" };
                let label = format!(" {}{} × ", buffer.name(), dirty);
                let width = UnicodeWidthStr::width(label.as_str()) as u16;
                if tab_x.saturating_add(width) > areas[0].right() {
                    tabs.push(Span::styled("›", Style::default().fg(theme::OVERLAY0)));
                    break;
                }
                self.tab_hits.push((tab_x, tab_x + width, index));
                self.tab_close_hits
                    .push((tab_x + width.saturating_sub(3), tab_x + width, index));
                tabs.push(Span::styled(
                    label,
                    if index == self.active {
                        Style::default()
                            .bg(theme::MAUVE)
                            .fg(theme::BASE)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT0)
                    },
                ));
                tab_x += width;
            }
            frame.render_widget(
                Paragraph::new(Line::from(tabs)).style(Style::default().bg(theme::MANTLE)),
                areas[0],
            );
        }

        if self.markdown_reading[self.active] {
            let rendered = crate::markdown::render(&self.current().text());
            self.top_line = self.top_line.min(
                rendered
                    .len()
                    .saturating_sub(usize::from(self.body.height.max(1))),
            );
            let visible = rendered
                .into_iter()
                .skip(self.top_line)
                .take(usize::from(self.body.height))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(visible).style(Style::default().bg(theme::BASE).fg(theme::TEXT)),
                self.body,
            );
            let status = self.modal_prompt_text().unwrap_or_else(|| {
                format!(
                    "{}   Markdown reading view   F1 help   Ctrl+Shift+M source",
                    self.current().name()
                )
            });
            if !self.focus_mode {
                frame.render_widget(
                    Paragraph::new(status)
                        .style(Style::default().bg(theme::SURFACE0).fg(theme::TEXT)),
                    areas[2],
                );
            }
            if self.help_visible {
                self.render_help(frame);
            }
            if self.explorer_prompt.is_some() {
                self.render_explorer_prompt(frame);
            }
            if self.close_armed.is_some() {
                self.render_close_prompt(frame);
            }
            if self.quick_open.is_some() {
                self.render_quick_open(frame);
            }
            if self.search.is_some() {
                self.render_search(frame);
            }
            if self.command_palette.is_some() {
                self.render_command_palette(frame);
            }
            if self.explorer_context_visible {
                self.render_explorer_context(frame);
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
        let left = if let Some(prompt) = self.modal_prompt_text() {
            prompt
        } else if let Some(path) = &self.path_prompt {
            format!("Save As: {path}_   Enter save  Esc cancel")
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
        if !self.focus_mode {
            frame.render_widget(
                Paragraph::new(status).style(Style::default().bg(theme::SURFACE0).fg(theme::TEXT)),
                areas[2],
            );
        }
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
        if self.explorer_prompt.is_some() {
            self.render_explorer_prompt(frame);
        }
        if self.close_armed.is_some() {
            self.render_close_prompt(frame);
        }
        if self.quick_open.is_some() {
            self.render_quick_open(frame);
        }
        if self.search.is_some() {
            self.render_search(frame);
        }
        if self.command_palette.is_some() {
            self.render_command_palette(frame);
        }
        if self.explorer_context_visible {
            self.render_explorer_context(frame);
        }
    }

    fn render_help(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.min(72);
        let height = area.height.min(29);
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
            "  Ctrl+N                                Create and open a new file",
            "  Ctrl+S / Ctrl+Shift+S                 Save / Save As",
            "  Ctrl+F                                Find",
            "  Search: Enter/Shift+Enter             Next/previous match",
            "  Search: Tab, Alt+C                    Field/case toggle",
            "  Search: Ctrl+R / Ctrl+Shift+R         Replace / replace all",
            "  Ctrl+P                                Quick Open workspace file",
            "  Ctrl+Shift+P                          Command Palette",
            "  Ctrl+W                                Close tab",
            "  Ctrl+E                                Toggle/focus file explorer",
            "  Explorer: arrows, Enter, Esc/Tab      Navigate/open/return",
            "  Explorer: N / Shift+N / R / D         New file/dir, rename/delete",
            "  Ctrl+Shift+M / F6                     Markdown reading view",
            "  F11                                   Toggle Focus Mode",
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

    fn render_explorer_prompt(&self, frame: &mut Frame) {
        let Some(prompt) = &self.explorer_prompt else {
            return;
        };
        let area = frame.area();
        let width = area.width.clamp(1, 64);
        let height = area.height.clamp(1, 5);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let title = match prompt.kind {
            ExplorerPromptKind::NewFile => " New file ",
            ExplorerPromptKind::NewDirectory => " New directory ",
            ExplorerPromptKind::Rename => " Rename ",
        };
        let text = format!("{}\n\nEnter confirm   Esc cancel", prompt.input);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(
                    Block::bordered()
                        .title(title)
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            popup,
        );
    }

    fn render_close_prompt(&self, frame: &mut Frame) {
        let Some(tab) = self.close_armed else {
            return;
        };
        let area = frame.area();
        let width = area.width.clamp(1, 60);
        let height = area.height.clamp(1, 7);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let name: String = self
            .buffers
            .get(tab)
            .map_or_else(|| "this file".into(), Buffer::name);
        let text = format!(
            "{name} has unsaved changes.\n\nClose it and discard those changes?\n\nY close   N/Esc cancel"
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(
                    Block::bordered()
                        .title(" Unsaved file ")
                        .border_style(Style::default().fg(theme::PEACH)),
                ),
            popup,
        );
    }

    fn render_quick_open(&self, frame: &mut Frame) {
        let Some(picker) = &self.quick_open else {
            return;
        };
        let area = frame.area();
        let width = area.width.clamp(1, 72);
        let height = area.height.clamp(1, 16);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 3,
            width,
            height,
        );
        let visible_count = usize::from(height.saturating_sub(4));
        let start = picker
            .selected()
            .saturating_sub(visible_count.saturating_sub(1));
        let mut lines = vec![
            Line::styled(
                format!("> {}_", picker.query()),
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::default(),
        ];
        for (index, path) in picker.matches().enumerate().skip(start).take(visible_count) {
            lines.push(Line::styled(
                format!(" {}", picker.display_path(path).display()),
                if index == picker.selected() {
                    Style::default().bg(theme::SURFACE1).fg(theme::MAUVE)
                } else {
                    Style::default().fg(theme::SUBTEXT0)
                },
            ));
        }
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE))
                .block(
                    Block::bordered()
                        .title(" Quick Open — Enter open, Esc cancel ")
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            popup,
        );
    }

    fn render_search(&self, frame: &mut Frame) {
        let Some(search) = &self.search else {
            return;
        };
        let area = frame.area();
        let width = area.width.clamp(1, 72);
        let height = area.height.clamp(1, 10);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 3,
            width,
            height,
        );
        let (current, total) = self
            .current()
            .search_status(&search.query, search.case_sensitive);
        let query_marker = if search.editing_replacement { " " } else { ">" };
        let replacement_marker = if search.editing_replacement { ">" } else { " " };
        let case = if search.case_sensitive {
            "case-sensitive"
        } else {
            "ignore case"
        };
        let lines = vec![
            Line::styled(
                format!("{query_marker} Find: {}", search.query),
                Style::default().fg(theme::TEXT),
            ),
            Line::styled(
                format!("{replacement_marker} Replace: {}", search.replacement),
                Style::default().fg(theme::TEXT),
            ),
            Line::default(),
            Line::styled(
                format!("{current}/{total} matches   {case}"),
                Style::default().fg(theme::SAPPHIRE),
            ),
            Line::styled(
                "Enter/Shift+Enter next/previous   Tab switch field   Alt+C case",
                Style::default().fg(theme::SUBTEXT0),
            ),
            Line::styled(
                "Ctrl+R replace   Ctrl+Shift+R replace all   Esc close",
                Style::default().fg(theme::SUBTEXT0),
            ),
        ];
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE))
                .block(
                    Block::bordered()
                        .title(" Find and Replace ")
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            popup,
        );
    }

    fn render_command_palette(&self, frame: &mut Frame) {
        let Some(palette) = &self.command_palette else {
            return;
        };
        let area = frame.area();
        let width = area.width.clamp(1, 72);
        let height = area.height.clamp(1, 18);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 3,
            width,
            height,
        );
        let visible_count = usize::from(height.saturating_sub(4));
        let start = palette
            .selected()
            .saturating_sub(visible_count.saturating_sub(1));
        let mut lines = vec![
            Line::styled(
                format!("> {}_", palette.query()),
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::default(),
        ];
        for (index, command) in palette
            .matches()
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_count)
        {
            lines.push(Line::styled(
                format!(" {}  [{}]", command.title(), command.id()),
                if index == palette.selected() {
                    Style::default().bg(theme::SURFACE1).fg(theme::MAUVE)
                } else {
                    Style::default().fg(theme::SUBTEXT0)
                },
            ));
        }
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE))
                .block(
                    Block::bordered()
                        .title(" Command Palette — Enter run, Esc cancel ")
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            popup,
        );
    }

    fn render_explorer_context(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.clamp(1, 44);
        let height = area.height.clamp(1, 9);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let name: String = self
            .explorer
            .selected_path()
            .and_then(Path::file_name)
            .map_or_else(|| "workspace".into(), |name| name.to_string_lossy().into());
        let text = format!(
            "{name}\n\nN       New file\nShift+N New directory\nR       Rename\nD       Delete\nEsc     Cancel"
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(
                    Block::bordered()
                        .title(" Explorer actions ")
                        .border_style(Style::default().fg(theme::SAPPHIRE)),
                ),
            popup,
        );
    }

    fn external_prompt_text(&self) -> Option<String> {
        self.external_prompt.map(|prompt| match prompt.change {
            ExternalChange::Modified => {
                "File changed on disk — R reloads (discarding editor edits), K keeps editor version"
                    .into()
            }
            ExternalChange::Deleted => {
                "File deleted on disk — R retries reload, K keeps buffer so Save can recreate it"
                    .into()
            }
            ExternalChange::None => String::new(),
        })
    }

    fn modal_prompt_text(&self) -> Option<String> {
        if let Some(path) = &self.delete_confirm {
            return Some(format!(
                "Delete {} permanently? Y confirm  N/Esc cancel",
                path.display()
            ));
        }
        self.external_prompt_text()
    }

    fn render_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        let inner_height = area.height.saturating_sub(2) as usize;
        self.explorer.ensure_visible(inner_height);
        let active_path = self.current().path().map(PathBuf::from);
        let start = self.explorer.scroll();
        let selected = self.explorer.selected();
        let focused = self.explorer.focused();
        let mut lines = Vec::with_capacity(inner_height);
        for (index, row) in self
            .explorer
            .rows()
            .iter()
            .enumerate()
            .skip(start)
            .take(inner_height)
        {
            let marker = if row.is_dir {
                if row.expanded {
                    "▾"
                } else {
                    "▸"
                }
            } else {
                " "
            };
            let name = row.path.file_name().map_or_else(
                || row.path.display().to_string(),
                |name| name.to_string_lossy().into(),
            );
            let label = format!("{}{} {name}", "  ".repeat(row.depth), marker);
            let active = active_path
                .as_deref()
                .is_some_and(|open| same_path(open, &row.path));
            let style = if focused && index == selected {
                Style::default()
                    .bg(theme::SURFACE1)
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD)
            } else if active {
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD)
            } else if row.is_dir {
                Style::default().fg(theme::BLUE)
            } else {
                Style::default().fg(theme::SUBTEXT0)
            };
            lines.push(Line::styled(label, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::MANTLE))
                .block(
                    Block::bordered()
                        .title(if focused { " Files • " } else { " Files " })
                        .border_style(Style::default().fg(if focused {
                            theme::MAUVE
                        } else {
                            theme::SURFACE1
                        })),
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

fn valid_entry_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod input_tests {
    use std::{
        fs,
        time::{Duration, Instant},
    };

    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{backend::TestBackend, Terminal};

    use crate::explorer::Explorer;

    use super::{control_letter, valid_entry_name, Editor};

    #[test]
    fn maps_raw_control_characters() {
        assert_eq!(control_letter('\u{3}'), Some('c'));
        assert_eq!(control_letter('\u{11}'), Some('q'));
        assert_eq!(control_letter('q'), None);
    }

    #[test]
    fn explorer_operations_require_a_single_safe_name() {
        assert!(valid_entry_name("notes.md"));
        assert!(valid_entry_name("folder name"));
        assert!(!valid_entry_name(""));
        assert!(!valid_entry_name("../outside"));
        assert!(!valid_entry_name("nested/file"));
        assert!(!valid_entry_name("."));
    }

    #[test]
    fn closing_bracket_dedents_to_the_opening_level() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert("{\n    ");
        editor
            .key(KeyEvent::new(KeyCode::Char('}'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(editor.current().text(), "{\n}");
    }

    #[test]
    fn ctrl_n_creates_opens_and_focuses_a_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ready.txt");
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor.explorer.set_focused(true);
        editor
            .key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL))
            .unwrap();
        for character in "ready.txt".chars() {
            editor
                .key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
                .unwrap();
        }
        editor
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(path.exists());
        assert_eq!(editor.current().path(), Some(path.as_path()));
        assert!(!editor.explorer.focused());
    }

    #[test]
    fn overflowing_tabs_keep_the_active_tab_visible() {
        let root = tempfile::tempdir().unwrap();
        let paths = (0..6)
            .map(|index| {
                let path = root.path().join(format!("document-{index}.txt"));
                fs::write(&path, index.to_string()).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let mut editor = Editor::new(paths);
        editor.active = 5;
        let backend = TestBackend::new(32, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert!(editor.tab_start > 0);
        assert!(editor.tab_hits.iter().any(|(_, _, tab)| *tab == 5));
    }

    #[test]
    fn dirty_tab_close_requires_explicit_confirmation() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert_typed("unsaved");
        editor.request_close_tab(0);
        assert_eq!(editor.close_armed, Some(0));
        assert_eq!(editor.current().text(), "unsaved");
        editor
            .key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .unwrap();
        assert!(editor.close_armed.is_none());
        assert_eq!(editor.current().text(), "unsaved");

        editor.request_close_tab(0);
        editor
            .key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
            .unwrap();
        assert!(editor.close_armed.is_none());
        assert_eq!(editor.current().text(), "");
    }

    #[test]
    fn focus_mode_hides_chrome_and_restores_explorer_layout() {
        let mut editor = Editor::new(Vec::new());
        editor.sidebar_visible = true;
        editor.toggle_focus_mode();
        assert!(editor.focus_mode);
        assert!(!editor.sidebar_visible);
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert_eq!(editor.body, ratatui::layout::Rect::new(0, 0, 80, 20));
        assert!(editor.tab_hits.is_empty());

        editor.toggle_focus_mode();
        assert!(!editor.focus_mode);
        assert!(editor.sidebar_visible);
    }

    #[test]
    fn ctrl_p_fuzzy_finds_and_opens_a_workspace_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("feature-notes.md");
        fs::write(&path, "# Feature").unwrap();
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor
            .key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();
        for character in "ftnotes".chars() {
            editor
                .key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
                .unwrap();
        }
        editor
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(editor.current().path(), Some(path.as_path()));
        assert!(editor.quick_open.is_none());
    }

    #[test]
    fn centered_search_dialog_replaces_current_and_all_matches() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert("One one");
        editor
            .key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        for character in "one".chars() {
            editor
                .key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
                .unwrap();
        }
        assert_eq!(editor.current().search_status("one", false), (1, 2));
        editor
            .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        for character in "two".chars() {
            editor
                .key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
                .unwrap();
        }
        editor
            .key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(editor.current().text(), "two one");
        editor
            .key(KeyEvent::new(
                KeyCode::Char('R'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ))
            .unwrap();
        assert_eq!(editor.current().text(), "two two");
    }

    #[test]
    fn command_palette_filters_and_runs_shared_commands() {
        let mut editor = Editor::new(Vec::new());
        editor
            .key(KeyEvent::new(
                KeyCode::Char('P'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ))
            .unwrap();
        for character in "focus".chars() {
            editor
                .key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
                .unwrap();
        }
        editor
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(editor.focus_mode);
        assert!(editor.command_palette.is_none());
    }

    #[test]
    fn right_clicking_explorer_item_surfaces_context_actions() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes.txt"), "notes").unwrap();
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor.sidebar_visible = true;
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        let area = editor.sidebar_area.unwrap();
        editor
            .handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: area.x + 1,
                row: area.y + 1,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        assert!(editor.explorer_context_visible);
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

    #[test]
    fn markdown_reading_view_keeps_keyboard_and_mouse_scroll() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("long.md");
        let text = (0..80)
            .map(|line| format!("Paragraph {line}\n\n"))
            .collect::<String>();
        fs::write(&path, text).unwrap();
        let mut editor = Editor::new(vec![path]);
        editor.markdown_reading[0] = true;
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();

        editor
            .key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert_eq!(editor.top_line, 1);

        editor
            .handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: editor.body.x,
                row: editor.body.y,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert_eq!(editor.top_line, 4);
    }

    #[test]
    fn save_as_refreshes_the_explorer() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("created.txt");
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor.path_prompt = Some(path.display().to_string());
        editor
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(path.exists());
        assert!(editor.explorer.rows().iter().any(|row| row.path == path));
    }

    #[test]
    fn clean_external_change_reloads_automatically() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("clean.txt");
        fs::write(&path, "before").unwrap();
        let mut editor = Editor::new(vec![path.clone()]);
        fs::write(path, "after with a different length").unwrap();
        assert!(editor.check_external_files());
        assert_eq!(editor.current().text(), "after with a different length");
        assert!(editor.external_prompt.is_none());
    }

    #[test]
    fn dirty_external_change_requires_choice_and_keep_preserves_text() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dirty.txt");
        fs::write(&path, "base").unwrap();
        let mut editor = Editor::new(vec![path.clone()]);
        editor.current_mut().insert_typed("editor");
        fs::write(path, "changed on disk with a different length").unwrap();
        assert!(editor.check_external_files());
        assert!(editor.external_prompt.is_some());
        editor
            .key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(editor.current().text(), "editorbase");
        assert!(editor.current().is_dirty());
        assert!(editor.external_prompt.is_none());
    }

    #[test]
    fn large_source_file_first_render_sanity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.rs");
        fs::write(&path, "fn example() { let value = 42; }\n".repeat(20_000)).unwrap();
        let mut editor = Editor::new(vec![path]);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let started = Instant::now();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
