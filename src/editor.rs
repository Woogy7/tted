use std::{
    collections::{HashMap, VecDeque},
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
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use serde_json::{json, Value};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme},
    parsing::SyntaxSet,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::agent::{AgentRequest, AgentServer};
use crate::agent_backend::{AgentBackend, BackendCommand, BackendEvent};
use crate::buffer::{Buffer, ExternalChange};
use crate::command::{Command, CommandPalette};
use crate::config::Config;
use crate::explorer::{Explorer, ExplorerAction};
use crate::git::GitService;
use crate::lsp::{Diagnostic, LspEvent, LspService};
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

#[derive(Clone, Copy)]
enum LspPromptKind {
    Rename,
    WorkspaceSymbols,
}

struct LspPrompt {
    kind: LspPromptKind,
    input: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitDirection {
    Right,
    Down,
}

struct SplitState {
    buffers: [usize; 2],
    active: usize,
    direction: SplitDirection,
}

struct AgentMessage {
    text: String,
    path: Option<PathBuf>,
    kind: AgentMessageKind,
}

#[derive(Clone, Copy)]
enum AgentMessageKind {
    Human,
    Agent,
    Activity,
    Error,
}

struct AgentDiskChange {
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

struct AgentApproval {
    id: Value,
    detail: String,
}

struct KeybindingsMenu {
    selected: usize,
    capturing: bool,
}

#[derive(Clone, Debug)]
enum BuiltinAgentStatus {
    Idle,
    Starting,
    SignIn,
    Ready,
    Working,
    LoginCode { url: String, code: String },
    Missing,
    Error(String),
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
    keybindings_menu: Option<KeybindingsMenu>,
    keybindings_area: Option<Rect>,
    close_armed: Option<usize>,
    path_prompt: Option<String>,
    help_visible: bool,
    gutter_width: u16,
    sidebar_visible: bool,
    focus_mode: bool,
    sidebar_before_focus: bool,
    sidebar_area: Option<Rect>,
    explorer: Explorer,
    git: GitService,
    explorer_prompt: Option<ExplorerPrompt>,
    delete_confirm: Option<PathBuf>,
    explorer_context_visible: bool,
    external_prompt: Option<ExternalPrompt>,
    git_discard_confirm: Option<PathBuf>,
    git_commit_prompt: Option<String>,
    config: Config,
    lsp: Option<LspService>,
    lsp_extension: Option<String>,
    lsp_dirty_since: Option<Instant>,
    problems_visible: bool,
    problems_selected: usize,
    problems_area: Option<Rect>,
    hover_popup: Option<String>,
    completions: Option<Vec<String>>,
    completion_selected: usize,
    navigation_history: Vec<(PathBuf, usize, usize)>,
    lsp_prompt: Option<LspPrompt>,
    split: Option<SplitState>,
    secondary_area: Option<Rect>,
    agent: Option<AgentServer>,
    agent_backend: Option<AgentBackend>,
    agent_backend_status: BuiltinAgentStatus,
    agent_last_prompt: Option<String>,
    agent_turn_diff: String,
    agent_stream_message: Option<usize>,
    agent_disk_before: HashMap<PathBuf, Vec<u8>>,
    agent_disk_changes: HashMap<PathBuf, AgentDiskChange>,
    agent_approval: Option<AgentApproval>,
    agent_activity: Vec<String>,
    agent_panel_visible: bool,
    agent_panel_focused: bool,
    agent_input: String,
    agent_input_cursor: usize,
    agent_input_anchor: Option<usize>,
    agent_transcript_anchor: Option<usize>,
    agent_transcript_cursor: Option<usize>,
    agent_transcript_text: String,
    agent_transcript_hits: Vec<(u16, usize, String)>,
    agent_messages: Vec<AgentMessage>,
    agent_scroll: usize,
    agent_prompts: VecDeque<String>,
    agent_modified: HashMap<u64, (usize, u64)>,
    agent_area: Option<Rect>,
    agent_input_area: Option<Rect>,
    agent_hits: Vec<(u16, PathBuf)>,
    syntaxes: SyntaxSet,
    theme: Theme,
    message: String,
    quit_armed: bool,
}

impl Editor {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let workspace_argument = paths.iter().find(|path| path.is_dir()).cloned();
        let workspace_root = workspace_argument
            .as_deref()
            .and_then(|path| path.canonicalize().ok())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let mut buffers = Vec::new();
        let mut message = String::new();
        for path in paths.into_iter().filter(|path| !path.is_dir()) {
            match Buffer::open(&path) {
                Ok(buffer) => buffers.push(buffer),
                Err(error) => message = format!("Could not open {}: {error}", path.display()),
            }
        }
        if buffers.is_empty() {
            buffers.push(Buffer::empty());
        }
        let markdown_reading = vec![false; buffers.len()];
        let config = Config::load(&workspace_root);
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let themes = syntect::highlighting::ThemeSet::load_defaults();
        let theme = themes
            .themes
            .get(&config.editor.theme)
            .or_else(|| themes.themes.get("base16-eighties.dark"))
            .expect("bundled syntax theme")
            .clone();
        let agent = (!cfg!(test) && config.agent.enabled)
            .then(|| AgentServer::start(AgentServer::default_path()).ok())
            .flatten();
        let mut editor = Self {
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
            keybindings_menu: None,
            keybindings_area: None,
            close_armed: None,
            path_prompt: None,
            help_visible: false,
            gutter_width: 0,
            sidebar_visible: workspace_argument.is_some(),
            focus_mode: false,
            sidebar_before_focus: false,
            sidebar_area: None,
            explorer: Explorer::with_options(
                workspace_root.clone(),
                config.explorer.show_hidden,
                config.explorer.show_build_directories,
                config.explorer.max_entries,
            ),
            git: GitService::new(workspace_root),
            explorer_prompt: None,
            delete_confirm: None,
            explorer_context_visible: false,
            external_prompt: None,
            git_discard_confirm: None,
            git_commit_prompt: None,
            config,
            lsp: None,
            lsp_extension: None,
            lsp_dirty_since: None,
            problems_visible: false,
            problems_selected: 0,
            problems_area: None,
            hover_popup: None,
            completions: None,
            completion_selected: 0,
            navigation_history: Vec::new(),
            lsp_prompt: None,
            split: None,
            secondary_area: None,
            agent,
            agent_backend: None,
            agent_backend_status: BuiltinAgentStatus::Idle,
            agent_last_prompt: None,
            agent_turn_diff: String::new(),
            agent_stream_message: None,
            agent_disk_before: HashMap::new(),
            agent_disk_changes: HashMap::new(),
            agent_approval: None,
            agent_activity: Vec::new(),
            agent_panel_visible: false,
            agent_panel_focused: false,
            agent_input: String::new(),
            agent_input_cursor: 0,
            agent_input_anchor: None,
            agent_transcript_anchor: None,
            agent_transcript_cursor: None,
            agent_transcript_text: String::new(),
            agent_transcript_hits: Vec::new(),
            agent_messages: Vec::new(),
            agent_scroll: 0,
            agent_prompts: VecDeque::new(),
            agent_modified: HashMap::new(),
            agent_area: None,
            agent_input_area: None,
            agent_hits: Vec::new(),
            syntaxes,
            theme,
            message,
            quit_armed: false,
        };
        editor.activate_lsp_for_current();
        editor
    }

    fn current(&self) -> &Buffer {
        &self.buffers[self.active]
    }

    fn indentation_unit(&self) -> String {
        if self.config.editor.use_spaces {
            " ".repeat(self.config.editor.tab_width.max(1))
        } else {
            "\t".into()
        }
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
            redraw |= self.git.tick();
            if let Some(message) = self.git.take_operation_message() {
                self.message = message;
                self.explorer.refresh();
                redraw = true;
            }
            if self
                .lsp_dirty_since
                .is_some_and(|started| started.elapsed() >= Duration::from_millis(150))
            {
                self.sync_lsp_change();
            }
            redraw |= self.poll_lsp();
            redraw |= self.poll_agent_backend();
            while let Some(request) = self.agent.as_ref().and_then(AgentServer::try_recv) {
                self.handle_agent_request(request);
                redraw = true;
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
                    || self.git_discard_confirm.is_some()
                {
                    return Ok(false);
                } else if let Some(message) = &mut self.git_commit_prompt {
                    message.push_str(text.trim_end_matches(['\r', '\n']));
                } else if self.agent_panel_focused {
                    self.insert_agent_input(text.trim_end_matches(['\r', '\n']));
                } else if let Some(prompt) = &mut self.lsp_prompt {
                    prompt.input.push_str(text.trim_end_matches(['\r', '\n']));
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
                } else if self.current().is_read_only() {
                    self.message = "Git views are read-only".into();
                } else {
                    self.current_mut().insert(&text.replace("\r\n", "\n"));
                    self.changed();
                    self.ensure_visible();
                }
            }
            Event::Mouse(mouse) if self.keybindings_menu.is_some() => {
                let area = self.keybindings_area.unwrap_or(self.body);
                let menu = self.keybindings_menu.as_mut().expect("keybindings menu");
                match mouse.kind {
                    MouseEventKind::ScrollUp => menu.selected = menu.selected.saturating_sub(1),
                    MouseEventKind::ScrollDown => {
                        menu.selected = (menu.selected + 1).min(Command::ALL.len() - 1)
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if area.contains((mouse.column, mouse.row).into()) =>
                    {
                        let visible = usize::from(area.height.saturating_sub(5));
                        let start = menu.selected.saturating_sub(visible.saturating_sub(1));
                        if mouse.row > area.y && mouse.row < area.bottom().saturating_sub(3) {
                            let selected = (start + usize::from(mouse.row - area.y - 1))
                                .min(Command::ALL.len() - 1);
                            menu.capturing = selected == menu.selected;
                            menu.selected = selected;
                        } else if mouse.row >= area.bottom().saturating_sub(3) {
                            menu.capturing = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::Mouse(_)
                if self.help_visible
                    || self.external_prompt.is_some()
                    || self.git_discard_confirm.is_some()
                    || self.git_commit_prompt.is_some()
                    || self.lsp_prompt.is_some()
                    || self.explorer_prompt.is_some()
                    || self.delete_confirm.is_some()
                    || self.close_armed.is_some()
                    || self.quick_open.is_some()
                    || self.search.is_some()
                    || self.command_palette.is_some()
                    || self.explorer_context_visible => {}
            Event::Mouse(mouse) => {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    && !self
                        .agent_area
                        .is_some_and(|area| area.contains((mouse.column, mouse.row).into()))
                {
                    self.agent_panel_focused = false;
                }
                match mouse.kind {
                    MouseEventKind::ScrollUp
                        if self.agent_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        self.agent_scroll = self.agent_scroll.saturating_add(3);
                    }
                    MouseEventKind::ScrollDown
                        if self.agent_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        self.agent_scroll = self.agent_scroll.saturating_sub(3);
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                        if self.agent_input_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        let area = self.agent_input_area.expect("Agent input area checked");
                        let position = agent_input_char_at(
                            &self.agent_input,
                            usize::from(area.width.saturating_sub(2).max(1)),
                            usize::from(area.height.saturating_sub(2).max(1)),
                            usize::from(mouse.row.saturating_sub(area.y + 1)),
                            usize::from(mouse.column.saturating_sub(area.x + 1)),
                        );
                        self.set_agent_input_cursor(position, true);
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                        if self
                            .agent_transcript_hits
                            .iter()
                            .any(|(row, _, _)| *row == mouse.row) =>
                    {
                        if let Some((_, offset, text)) = self
                            .agent_transcript_hits
                            .iter()
                            .find(|(row, _, _)| *row == mouse.row)
                        {
                            let text_column = usize::from(
                                mouse
                                    .column
                                    .saturating_sub(self.agent_area.expect("Agent area").x + 7),
                            );
                            self.agent_transcript_cursor =
                                Some(*offset + text_column.min(text.chars().count()));
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.agent_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        let area = self.agent_area.expect("Agent area checked");
                        if let Some(input_area) = self.agent_input_area.filter(|input_area| {
                            input_area.contains((mouse.column, mouse.row).into())
                        }) {
                            let position = agent_input_char_at(
                                &self.agent_input,
                                usize::from(input_area.width.saturating_sub(2).max(1)),
                                usize::from(input_area.height.saturating_sub(2).max(1)),
                                usize::from(mouse.row.saturating_sub(input_area.y + 1)),
                                usize::from(mouse.column.saturating_sub(input_area.x + 1)),
                            );
                            self.agent_panel_focused = true;
                            self.set_agent_input_cursor(position, false);
                            return Ok(false);
                        }
                        if let Some((_, offset, text)) = self
                            .agent_transcript_hits
                            .iter()
                            .find(|(row, _, _)| *row == mouse.row)
                        {
                            let text_column = usize::from(mouse.column.saturating_sub(area.x + 7));
                            let position = *offset + text_column.min(text.chars().count());
                            self.agent_transcript_anchor = Some(position);
                            self.agent_transcript_cursor = Some(position);
                            self.agent_panel_focused = true;
                            return Ok(false);
                        }
                        if self.agent_approval.is_some() {
                            self.answer_agent_approval(mouse.column < area.x + area.width / 2);
                            return Ok(false);
                        }
                        match &self.agent_backend_status {
                            BuiltinAgentStatus::SignIn => {
                                if let Some(backend) = &self.agent_backend {
                                    backend.send(BackendCommand::Login);
                                    self.agent_backend_status = BuiltinAgentStatus::Starting;
                                }
                                return Ok(false);
                            }
                            BuiltinAgentStatus::Missing => {
                                self.message =
                                "Install Codex: curl -fsSL https://chatgpt.com/codex/install.sh | sh"
                                    .into();
                                return Ok(false);
                            }
                            BuiltinAgentStatus::LoginCode { code, .. } => {
                                let code = code.clone();
                                self.copy_to_terminal_clipboard(&code)?;
                                self.clipboard = Some(code);
                                self.message = "Copied the Codex sign-in code".into();
                                return Ok(false);
                            }
                            _ => {}
                        }
                        if mouse.row == area.bottom().saturating_sub(2) {
                            let relative = mouse.column.saturating_sub(area.x);
                            if relative < 8 {
                                self.stop_agent();
                            } else if relative < 16 {
                                self.retry_agent();
                            } else if relative < 22 {
                                self.new_agent_conversation();
                            } else {
                                self.clear_agent_chat();
                            }
                        } else if mouse.row == area.bottom().saturating_sub(1) {
                            let relative = mouse.column.saturating_sub(area.x);
                            if relative < 8 {
                                self.open_agent_diff();
                            } else if relative < 17 {
                                self.accept_agent_changes();
                            } else {
                                self.revert_agent_changes();
                            }
                        } else if mouse.row >= area.bottom().saturating_sub(4) {
                            self.agent_panel_focused = true;
                        } else if let Some((_, path)) = self
                            .agent_hits
                            .iter()
                            .find(|(row, _)| *row == mouse.row)
                            .cloned()
                        {
                            self.open_path(path);
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.secondary_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        self.focus_next_split();
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.problems_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        let area = self.problems_area.expect("Problems area checked");
                        if mouse.row > area.y && mouse.row < area.bottom().saturating_sub(1) {
                            let visible = usize::from(area.height.saturating_sub(2));
                            let start = self
                                .problems_selected
                                .saturating_sub(visible.saturating_sub(1));
                            self.open_problem(start + usize::from(mouse.row - area.y - 1));
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right)
                        if self.sidebar_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
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
                        if self.sidebar_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        let height = self
                            .sidebar_area
                            .map_or(1, |area| usize::from(area.height.saturating_sub(2).max(1)));
                        self.explorer.scroll_by(-3, height);
                    }
                    MouseEventKind::ScrollDown
                        if self.sidebar_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
                    {
                        let height = self
                            .sidebar_area
                            .map_or(1, |area| usize::from(area.height.saturating_sub(2).max(1)));
                        self.explorer.scroll_by(3, height);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.sidebar_area.is_some_and(|area| {
                            area.contains((mouse.column, mouse.row).into())
                        }) =>
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
                        self.agent_panel_focused = false;
                        self.explorer.set_focused(false);
                        let line = self.top_line + usize::from(mouse.row - self.body.y);
                        let col = self.left_col
                            + usize::from(
                                mouse.column.saturating_sub(self.body.x + self.gutter_width),
                            );
                        let select = matches!(mouse.kind, MouseEventKind::Drag(_));
                        self.current_mut()
                            .set_cursor_line_screen_col(line, col, select);
                        self.ensure_visible();
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.markdown_reading[self.active]
                            && self.body.contains((mouse.column, mouse.row).into()) =>
                    {
                        self.agent_panel_focused = false;
                        self.explorer.set_focused(false);
                        let rendered_line =
                            self.top_line + usize::from(mouse.row.saturating_sub(self.body.y));
                        let rendered_column = usize::from(mouse.column.saturating_sub(self.body.x));
                        self.toggle_markdown_task(rendered_line, rendered_column);
                    }
                    MouseEventKind::ScrollUp => self.top_line = self.top_line.saturating_sub(3),
                    MouseEventKind::ScrollDown => {
                        let max_top = if self.markdown_reading[self.active] {
                            self.markdown_max_top()
                        } else {
                            self.document_max_top()
                        };
                        self.top_line = (self.top_line + 3).min(max_top)
                    }
                    _ => {}
                }
            }
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
        if self.agent_panel_focused && !(ctrl && key.code == KeyCode::Char('q')) {
            return self.agent_panel_key(key);
        }
        if self.hover_popup.is_some() {
            self.hover_popup = None;
            return Ok(false);
        }
        if self.completions.is_some() {
            match key.code {
                KeyCode::Esc => self.completions = None,
                KeyCode::Up => {
                    self.completion_selected = self.completion_selected.saturating_sub(1)
                }
                KeyCode::Down => {
                    let max = self
                        .completions
                        .as_ref()
                        .map_or(0, |items| items.len().saturating_sub(1));
                    self.completion_selected = (self.completion_selected + 1).min(max);
                }
                KeyCode::Enter => {
                    let selected = self
                        .completions
                        .as_ref()
                        .and_then(|items| items.get(self.completion_selected))
                        .cloned();
                    self.completions = None;
                    if let Some(text) = selected {
                        self.current_mut().insert_typed(&text);
                        self.changed();
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.lsp_prompt.is_some() {
            return self.lsp_prompt_key(key);
        }
        if self.keybindings_menu.is_some() {
            return self.keybindings_menu_key(key);
        }
        if let Some(command) = self
            .config
            .keybindings
            .get(&key_event_name(&key))
            .and_then(|id| Command::from_id(id))
        {
            return self.execute_command(command);
        }
        if self.help_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
                self.help_visible = false;
            }
            return Ok(false);
        }
        if key.code == KeyCode::F(1) {
            return self.execute_command(Command::ShowHelp);
        }
        if key.code == KeyCode::F(2) {
            self.command_palette = Some(CommandPalette::new());
            return Ok(false);
        }
        if key.code == KeyCode::F(3) {
            return self.execute_command(Command::OpenKeybindings);
        }
        if key.code == KeyCode::F(8) {
            self.problems_visible = true;
            return self.jump_to_problem(1);
        }
        if key.code == KeyCode::F(9) {
            return self.execute_command(Command::ToggleAgentPanel);
        }
        if self.external_prompt.is_some() {
            return self.external_prompt_key(key);
        }
        if self.git_discard_confirm.is_some() {
            return self.git_discard_key(key);
        }
        if self.git_commit_prompt.is_some() {
            return self.git_commit_key(key);
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
        if ctrl && matches!(key.code, KeyCode::Char('g') | KeyCode::Char('G')) {
            if self.agent_panel_visible {
                self.agent_panel_focused = true;
                return Ok(false);
            }
            return self.execute_command(Command::ToggleAgentPanel);
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
        if self.current().is_read_only()
            && ctrl
            && matches!(key.code, KeyCode::Char('s' | 'x' | 'v' | 'z' | 'y'))
        {
            self.message = "Git views are read-only".into();
            return Ok(false);
        }
        if ctrl {
            match key.code {
                KeyCode::Char('a') => {
                    self.current_mut().select_all();
                    self.ensure_visible();
                    self.message = "Selected all text".into();
                }
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
        if self.current().is_read_only()
            && matches!(
                key.code,
                KeyCode::Char(_)
                    | KeyCode::Enter
                    | KeyCode::Tab
                    | KeyCode::BackTab
                    | KeyCode::Backspace
                    | KeyCode::Delete
            )
        {
            self.message = "Git views are read-only".into();
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
                    let width = self.config.editor.tab_width.max(1);
                    self.current_mut().unindent_current_line(width);
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
                    format!("{indent}{}", self.indentation_unit())
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
                let width = self.config.editor.tab_width.max(1);
                let insertion = if self.config.editor.use_spaces {
                    let col = self.current().cursor_screen_col();
                    " ".repeat(width - (col % width))
                } else {
                    "\t".into()
                };
                self.current_mut().insert(&insertion);
                self.changed();
            }
            KeyCode::BackTab => {
                let width = self.config.editor.tab_width.max(1);
                self.current_mut().unindent_current_line(width);
                self.changed();
            }
            KeyCode::Backspace => {
                let width = self.config.editor.tab_width.max(1);
                self.current_mut().smart_backspace(width);
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

    fn keybindings_menu_key(&mut self, key: KeyEvent) -> Result<bool> {
        let capturing = self
            .keybindings_menu
            .as_ref()
            .is_some_and(|menu| menu.capturing);
        if capturing {
            if key.code == KeyCode::Esc {
                if let Some(menu) = &mut self.keybindings_menu {
                    menu.capturing = false;
                }
                return Ok(false);
            }
            let name = key_event_name(&key);
            if name.is_empty()
                || matches!(key.code, KeyCode::Char(_))
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                self.message = "Use Ctrl, Alt, or a function/navigation key for shortcuts".into();
                return Ok(false);
            }
            let selected = self
                .keybindings_menu
                .as_ref()
                .map_or(0, |menu| menu.selected);
            let command = Command::ALL[selected.min(Command::ALL.len() - 1)];
            self.config
                .keybindings
                .retain(|_, command_id| command_id != command.id());
            let replaced = self
                .config
                .keybindings
                .insert(name.clone(), command.id().into());
            self.persist_keybindings();
            if let Some(menu) = &mut self.keybindings_menu {
                menu.capturing = false;
            }
            self.message = if replaced.is_some() {
                format!("Assigned {name}; replaced its previous action")
            } else {
                format!("Assigned {name} to {}", command.title())
            };
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc | KeyCode::F(3) => self.keybindings_menu = None,
            KeyCode::Up => {
                if let Some(menu) = &mut self.keybindings_menu {
                    menu.selected = menu.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(menu) = &mut self.keybindings_menu {
                    menu.selected = (menu.selected + 1).min(Command::ALL.len() - 1);
                }
            }
            KeyCode::Home => self.keybindings_menu.as_mut().expect("menu").selected = 0,
            KeyCode::End => {
                self.keybindings_menu.as_mut().expect("menu").selected = Command::ALL.len() - 1
            }
            KeyCode::Enter => self.keybindings_menu.as_mut().expect("menu").capturing = true,
            KeyCode::Backspace | KeyCode::Delete => {
                let selected = self.keybindings_menu.as_ref().expect("menu").selected;
                let command = Command::ALL[selected];
                let before = self.config.keybindings.len();
                self.config
                    .keybindings
                    .retain(|_, command_id| command_id != command.id());
                if self.config.keybindings.len() != before {
                    self.persist_keybindings();
                    self.message = format!("Reset {} to its default", command.title());
                } else {
                    self.message = "This action has no custom shortcut".into();
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn persist_keybindings(&mut self) {
        if let Err(error) = self.config.save_keybindings(self.explorer.root()) {
            self.message = format!("Could not save keybindings: {error}");
        }
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
                if self.current().is_read_only() {
                    self.message = "Git views are read-only".into();
                } else if command == Command::SaveAs || self.current().path().is_none() {
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
                            self.git.request_refresh();
                            self.message = "Saved".into();
                            if let (Some(lsp), Some(path)) = (&self.lsp, self.current().path()) {
                                lsp.save(path.to_path_buf());
                            }
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
                if self.current().is_read_only() {
                    self.message = "Find and Replace is unavailable in read-only Git views".into();
                } else if self.markdown_reading[self.active] {
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
                    self.activate_lsp_for_current();
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
            Command::GitStatus => {
                let text = self.git.snapshot().status_text();
                self.open_read_only("Git Status", text);
            }
            Command::GitCurrentFileDiff => {
                let Some(path) = self.current().path().map(PathBuf::from) else {
                    self.message = "Open a saved file before requesting its Git diff".into();
                    return Ok(false);
                };
                let text = self.git.snapshot().file_diff(&path);
                self.open_read_only("Current File.diff", text);
            }
            Command::GitWorkspaceDiff => {
                let text = self.git.snapshot().workspace_diff();
                self.open_read_only("Workspace.diff", text);
            }
            Command::GitStageCurrentFile => self.start_git_file_operation("stage"),
            Command::GitUnstageCurrentFile => self.start_git_file_operation("unstage"),
            Command::GitDiscardCurrentFile => {
                if self.current().is_dirty() {
                    self.message = "Save or discard editor changes before Git discard".into();
                } else if let Some(path) = self.current().path().map(PathBuf::from) {
                    if self.git.snapshot().decoration(&path) == Some('?') {
                        self.message = "Git discard does not delete untracked files".into();
                    } else {
                        self.git_discard_confirm = Some(path);
                    }
                } else {
                    self.message = "Open a saved repository file first".into();
                }
            }
            Command::GitCommit => {
                self.git_commit_prompt = Some(String::new());
                self.message.clear();
            }
            Command::ToggleProblems => self.problems_visible = !self.problems_visible,
            Command::LspHover => self.request_lsp("hover"),
            Command::LspDefinition => self.request_lsp("definition"),
            Command::LspCompletion => self.request_lsp("completion"),
            Command::LspBack => {
                if let Some((path, line, column)) = self.navigation_history.pop() {
                    self.open_path(path);
                    self.current_mut().set_cursor_line_col(line, column, false);
                    self.ensure_visible();
                } else {
                    self.message = "Navigation history is empty".into();
                }
            }
            Command::LspReferences => self.request_lsp("references"),
            Command::LspRename => {
                self.lsp_prompt = Some(LspPrompt {
                    kind: LspPromptKind::Rename,
                    input: String::new(),
                })
            }
            Command::LspCodeActions => self.request_lsp("code actions"),
            Command::LspFormat => self.request_lsp("format"),
            Command::LspDocumentSymbols => self.request_lsp("document symbols"),
            Command::LspWorkspaceSymbols => {
                self.lsp_prompt = Some(LspPrompt {
                    kind: LspPromptKind::WorkspaceSymbols,
                    input: String::new(),
                })
            }
            Command::LspSignature => self.request_lsp("signature"),
            Command::LspRestart => {
                self.lsp = None;
                self.lsp_extension = None;
                self.activate_lsp_for_current();
            }
            Command::SplitRight => self.create_split(SplitDirection::Right),
            Command::SplitDown => self.create_split(SplitDirection::Down),
            Command::CloseSplit => {
                if self.split.take().is_some() {
                    self.message = "Split closed".into();
                } else {
                    self.message = "No split is open".into();
                }
            }
            Command::FocusNextSplit => self.focus_next_split(),
            Command::ToggleAgentPanel => {
                self.agent_panel_visible = !self.agent_panel_visible;
                self.agent_panel_focused = self.agent_panel_visible;
                if self.agent_panel_visible {
                    self.ensure_agent_backend();
                }
            }
            Command::AgentViewDiff => {
                self.open_agent_diff();
            }
            Command::AgentAccept => self.accept_agent_changes(),
            Command::AgentRevert => self.revert_agent_changes(),
            Command::AgentAsk => {
                self.agent_panel_visible = true;
                self.agent_panel_focused = true;
                self.ensure_agent_backend();
            }
            Command::AgentExplainSelection => self.enqueue_context_prompt("Explain this selection"),
            Command::AgentRefactorSelection => {
                self.enqueue_context_prompt("Refactor this selection")
            }
            Command::AgentWriteTests => {
                self.enqueue_context_prompt("Write tests for the current context")
            }
            Command::AgentReviewDiff => {
                self.enqueue_agent_prompt("Review the current Git diff".into())
            }
            Command::OpenKeybindings => {
                self.keybindings_menu = Some(KeybindingsMenu {
                    selected: 0,
                    capturing: false,
                });
                self.message.clear();
            }
            Command::ReloadConfig => {
                let root = self.explorer.root().to_path_buf();
                self.config = Config::load(&root);
                self.explorer = Explorer::with_options(
                    root,
                    self.config.explorer.show_hidden,
                    self.config.explorer.show_build_directories,
                    self.config.explorer.max_entries,
                );
                let themes = syntect::highlighting::ThemeSet::load_defaults();
                if let Some(theme) = themes
                    .themes
                    .get(&self.config.editor.theme)
                    .or_else(|| themes.themes.get("base16-eighties.dark"))
                {
                    self.theme = theme.clone();
                }
                self.lsp = None;
                self.lsp_extension = None;
                self.activate_lsp_for_current();
                self.message = "Configuration reloaded".into();
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

    fn start_git_file_operation(&mut self, operation: &str) {
        if self.current().is_dirty() {
            self.message = "Save the current file before changing its Git state".into();
            return;
        }
        let Some(path) = self.current().path().map(PathBuf::from) else {
            self.message = "Open a saved repository file first".into();
            return;
        };
        let started = match operation {
            "stage" => self.git.stage(&path),
            "unstage" => self.git.unstage(&path),
            _ => false,
        };
        self.message = if started {
            format!("Git {operation} started…")
        } else {
            "Git operation unavailable or already running".into()
        };
    }

    fn git_discard_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                let path = self.git_discard_confirm.take().expect("Git discard path");
                self.message = if self.git.discard(&path) {
                    "Git discard started…".into()
                } else {
                    "Git discard unavailable or another operation is running".into()
                };
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.git_discard_confirm = None;
                self.message = "Git discard cancelled".into();
            }
            _ => {}
        }
        Ok(false)
    }

    fn git_commit_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.git_commit_prompt = None;
                self.message = "Git commit cancelled".into();
            }
            KeyCode::Backspace => {
                self.git_commit_prompt
                    .as_mut()
                    .expect("commit prompt")
                    .pop();
            }
            KeyCode::Enter => {
                let message = self.git_commit_prompt.take().unwrap_or_default();
                if message.trim().is_empty() {
                    self.message = "Commit message cannot be empty".into();
                    self.git_commit_prompt = Some(message);
                } else {
                    self.message = if self.git.commit(message) {
                        "Git commit started…".into()
                    } else {
                        "Git commit unavailable or another operation is running".into()
                    };
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => self
                .git_commit_prompt
                .as_mut()
                .expect("commit prompt")
                .push(character),
            _ => {}
        }
        Ok(false)
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
                        self.git.request_refresh();
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
                        self.git.request_refresh();
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
                            self.git.request_refresh();
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
        self.split = None;
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
            self.update_active_split_buffer();
            self.reset_view();
            self.activate_lsp_for_current();
            return;
        }
        match Buffer::open(&path) {
            Ok(buffer) => {
                self.buffers.push(buffer);
                self.markdown_reading.push(false);
                self.active = self.buffers.len() - 1;
                self.update_active_split_buffer();
                self.message = format!("Opened {}", path.display());
                self.reset_view();
                self.activate_lsp_for_current();
            }
            Err(error) => self.message = format!("Could not open {}: {error}", path.display()),
        }
    }

    fn create_split(&mut self, direction: SplitDirection) {
        self.split = Some(SplitState {
            buffers: [self.active, self.active],
            active: 0,
            direction,
        });
        self.message = if direction == SplitDirection::Right {
            "Split right"
        } else {
            "Split down"
        }
        .into();
    }

    fn focus_next_split(&mut self) {
        let Some(split) = &mut self.split else {
            self.message = "No split is open".into();
            return;
        };
        split.buffers[split.active] = self.active;
        split.active = 1 - split.active;
        let pane = split.active;
        let buffer = split.buffers[pane];
        self.active = buffer.min(self.buffers.len().saturating_sub(1));
        self.reset_view();
        self.activate_lsp_for_current();
        self.message = format!("Focused split {}", pane + 1);
    }

    fn update_active_split_buffer(&mut self) {
        if let Some(split) = &mut self.split {
            split.buffers[split.active] = self.active;
        }
    }

    fn request_lsp(&mut self, request: &str) {
        let Some(path) = self.current().path().map(PathBuf::from) else {
            self.message = "LSP actions require a saved file".into();
            return;
        };
        let (line, column) = self.current().cursor_line_col();
        let Some(lsp) = &self.lsp else {
            self.message = "No language server configured for this file".into();
            return;
        };
        match request {
            "hover" => lsp.hover(path, line, column),
            "definition" => lsp.definition(path, line, column),
            "completion" => lsp.completion(path, line, column),
            "references" => lsp.references(path, line, column),
            "code actions" => lsp.code_actions(path, line, column),
            "format" => lsp.formatting(path),
            "document symbols" => lsp.document_symbols(path),
            "signature" => lsp.signature(path, line, column),
            _ => return,
        }
        self.message = format!("LSP {request} requested…");
    }

    fn lsp_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.lsp_prompt = None;
                self.message = "Language action cancelled".into();
            }
            KeyCode::Backspace => {
                self.lsp_prompt.as_mut().expect("LSP prompt").input.pop();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => self
                .lsp_prompt
                .as_mut()
                .expect("LSP prompt")
                .input
                .push(character),
            KeyCode::Enter => {
                let prompt = self.lsp_prompt.take().expect("LSP prompt");
                if prompt.input.trim().is_empty() {
                    self.message = "A non-empty value is required".into();
                    self.lsp_prompt = Some(prompt);
                    return Ok(false);
                }
                let Some(lsp) = &self.lsp else {
                    self.message = "No language server configured".into();
                    return Ok(false);
                };
                match prompt.kind {
                    LspPromptKind::Rename => {
                        let Some(path) = self.current().path().map(PathBuf::from) else {
                            return Ok(false);
                        };
                        let (line, column) = self.current().cursor_line_col();
                        lsp.rename(path, line, column, prompt.input);
                    }
                    LspPromptKind::WorkspaceSymbols => lsp.workspace_symbols(prompt.input),
                }
                self.message = "Language request started…".into();
            }
            _ => {}
        }
        Ok(false)
    }

    fn jump_to_problem(&mut self, delta: isize) -> Result<bool> {
        let items = self.current_diagnostics();
        if items.is_empty() {
            self.message = "No diagnostics".into();
            return Ok(false);
        }
        self.problems_selected = self
            .problems_selected
            .saturating_add_signed(delta)
            .min(items.len() - 1);
        self.open_problem(self.problems_selected);
        Ok(false)
    }

    fn open_problem(&mut self, index: usize) {
        let items = self.current_diagnostics();
        let Some(item) = items.get(index).cloned() else {
            return;
        };
        self.problems_selected = index;
        self.open_path(item.path);
        self.current_mut()
            .set_cursor_line_col(item.line, item.column, false);
        self.ensure_visible();
        self.message = item.message;
    }

    fn open_read_only(&mut self, name: &str, text: String) {
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.is_read_only() && buffer.name() == name)
        {
            self.buffers[index] = Buffer::read_only(name, text);
            self.active = index;
        } else {
            self.buffers.push(Buffer::read_only(name, text));
            self.markdown_reading.push(false);
            self.active = self.buffers.len() - 1;
        }
        self.message = format!("Opened {name} (read-only)");
        self.reset_view();
    }

    fn copy_to_terminal_clipboard(&self, text: &str) -> Result<()> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(text);
        write!(io::stdout(), "\x1b]52;c;{encoded}\x07")?;
        io::stdout().flush()?;
        Ok(())
    }

    fn changed(&mut self) {
        self.quit_armed = false;
        self.lsp_dirty_since.get_or_insert_with(Instant::now);
        self.ensure_visible();
    }

    fn activate_lsp_for_current(&mut self) {
        let Some(path) = self.current().path().map(PathBuf::from) else {
            return;
        };
        let Some(extension) = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            return;
        };
        if self.lsp_extension.as_deref() == Some(&extension) {
            return;
        }
        self.lsp = None;
        self.lsp_extension = None;
        let Some(server) = self.config.language_server(&path).cloned() else {
            return;
        };
        match LspService::start(self.explorer.root().to_path_buf(), server) {
            Ok(service) => {
                self.lsp = Some(service);
                self.lsp_extension = Some(extension);
                self.message = "Language server starting…".into();
            }
            Err(error) => self.message = format!("Could not start language service: {error}"),
        }
    }

    fn sync_lsp_change(&mut self) {
        self.lsp_dirty_since = None;
        let Some(path) = self.current().path().map(PathBuf::from) else {
            return;
        };
        let text = self.current().text();
        let version = self.current().revision() as i64 + 1;
        if let Some(lsp) = &self.lsp {
            lsp.change(path, version, text);
        }
    }

    fn poll_lsp(&mut self) -> bool {
        let events = self.lsp.as_mut().map(LspService::poll).unwrap_or_default();
        let changed = !events.is_empty();
        for event in events {
            match event {
                LspEvent::Ready => {
                    if let Some(path) = self.current().path().map(PathBuf::from) {
                        let text = self.current().text();
                        if let Some(lsp) = &self.lsp {
                            lsp.open(path, text);
                        }
                    }
                    self.message = "Language server ready".into();
                }
                LspEvent::Diagnostics { .. } => {
                    let count = self.lsp.as_ref().map_or(0, |lsp| lsp.diagnostics().count());
                    self.problems_selected = self.problems_selected.min(count.saturating_sub(1));
                }
                LspEvent::Hover(text) => {
                    self.hover_popup = Some(if text.is_empty() {
                        "No hover information".into()
                    } else {
                        text
                    });
                }
                LspEvent::Definition { path, line, column } => {
                    if let Some(current) = self.current().path().map(PathBuf::from) {
                        let (current_line, current_column) = self.current().cursor_line_col();
                        self.navigation_history
                            .push((current, current_line, current_column));
                    }
                    self.open_path(path);
                    self.current_mut().set_cursor_line_col(line, column, false);
                    self.ensure_visible();
                }
                LspEvent::Completions(items) => {
                    self.completion_selected = 0;
                    self.completions = Some(items);
                }
                LspEvent::Locations(locations) => {
                    let text = if locations.is_empty() {
                        "No references found.\n".into()
                    } else {
                        locations
                            .iter()
                            .map(|(path, line, column)| {
                                format!("{}:{}:{}\n", path.display(), line + 1, column + 1)
                            })
                            .collect()
                    };
                    self.open_read_only("References", text);
                }
                LspEvent::WorkspaceEdits(changes) => {
                    let mut files = 0;
                    for (path, edits) in changes {
                        self.open_path(path);
                        self.current_mut().apply_text_edits(&edits);
                        files += 1;
                    }
                    if files > 0 {
                        self.changed();
                    }
                    self.message = format!("Applied language-server edits to {files} file(s)");
                }
                LspEvent::Information(text) => self.hover_popup = Some(text),
            }
        }
        changed
    }

    fn current_diagnostics(&self) -> Vec<Diagnostic> {
        self.lsp
            .as_ref()
            .map(LspService::all_diagnostics)
            .unwrap_or_default()
    }

    fn handle_agent_request(&mut self, request: AgentRequest) {
        let method = request.method.clone();
        let params = request.params.clone();
        let is_write = matches!(
            method.as_str(),
            "edit.apply"
                | "edit.apply_batch"
                | "editor.edit_text"
                | "editor.open"
                | "editor.focus_range"
        );
        let allowed = if method == "file.create" {
            self.config.agent.allow_file_create
        } else if method == "file.delete" {
            self.config.agent.allow_file_delete
        } else if method == "command.run" {
            self.config.agent.allow_commands
        } else if is_write {
            self.config.agent.allow_write
        } else {
            self.config.agent.allow_read
        };
        let result = if !allowed {
            Err(format!("permission denied for {method}"))
        } else {
            self.execute_agent_method(&method, &params)
        };
        self.agent_activity.push(format!(
            "{} {}",
            if result.is_ok() { "✓" } else { "!" },
            method
        ));
        if self.agent_activity.len() > 100 {
            self.agent_activity.remove(0);
        }
        self.message = format!("Agent: {method}");
        request.reply(result);
    }

    fn execute_agent_method(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "workspace.info" => Ok(json!({"root":self.explorer.root(),"socket":self.agent.as_ref().map(|agent|agent.path()),"buffers":self.buffers.len()})),
            "workspace.files" => Ok(json!(collect_workspace_files(self.explorer.root(), 20_000))),
            "buffer.list" => Ok(Value::Array(self.buffers.iter().map(|buffer| json!({"id":buffer.id(),"path":buffer.path(),"name":buffer.name(),"revision":buffer.revision(),"dirty":buffer.is_dirty(),"read_only":buffer.is_read_only()})).collect())),
            "buffer.read" => { let buffer=self.agent_buffer(params)?; Ok(json!({"id":buffer.id(),"revision":buffer.revision(),"text":buffer.text()})) }
            "buffer.revision" => { let buffer=self.agent_buffer(params)?; Ok(json!({"id":buffer.id(),"revision":buffer.revision()})) }
            "editor.current_file" => Ok(json!({"buffer_id":self.current().id(),"path":self.current().path(),"revision":self.current().revision()})),
            "editor.cursor" => { let (line,column)=self.current().cursor_line_col(); Ok(json!({"line":line,"column":column,"char_offset":self.current().cursor()})) }
            "editor.selection" => Ok(json!({"range":self.current().selection(),"text":self.current().selected_text()})),
            "editor.open" => { let path=self.safe_agent_path(params)?; self.open_path(path); Ok(json!({"buffer_id":self.current().id()})) }
            "editor.focus_range" => { let id=required_u64(params,"buffer_id")?; let line=required_u64(params,"line")? as usize; let column=required_u64(params,"column")? as usize; let index=self.buffers.iter().position(|buffer|buffer.id()==id).ok_or("unknown buffer")?; self.active=index; self.update_active_split_buffer(); self.current_mut().set_cursor_line_col(line,column,false); self.ensure_visible(); Ok(json!({"focused":true})) }
            "edit.apply" => { let id=required_u64(params,"buffer_id")?; let revision=required_u64(params,"revision")?; let start=required_u64(params,"start")? as usize; let end=required_u64(params,"end")? as usize; let text=params.get("text").and_then(Value::as_str).ok_or("missing text")?; let buffer=self.buffers.iter_mut().find(|buffer|buffer.id()==id).ok_or("unknown buffer")?; let revision=buffer.apply_agent_edit(revision,start,end,text).map_err(str::to_owned)?; let tracked=self.agent_modified.entry(id).or_insert((0,revision)); tracked.0+=1; tracked.1=revision; self.changed(); Ok(json!({"revision":revision})) }
            "editor.edit_text" => self.apply_agent_text_operation(params),
            "diagnostics.list" => Ok(json!(self.current_diagnostics().iter().map(|item|json!({"path":item.path,"line":item.line,"column":item.column,"severity":item.severity,"message":item.message})).collect::<Vec<_>>())),
            "git.status" => Ok(json!({"text":self.git.snapshot().status_text()})),
            "git.diff" => Ok(json!({"text":self.git.snapshot().workspace_diff()})),
            "agent.next_prompt" => Ok(json!({"prompt":self.agent_prompts.pop_front()})),
            "agent.respond" => { let text=params.get("text").and_then(Value::as_str).ok_or("missing text")?.to_owned(); let append=params.get("append").and_then(Value::as_bool).unwrap_or(false); if append { if let Some(last)=self.agent_messages.last_mut() { last.text.push_str(&text); } else { self.agent_messages.push(AgentMessage{text,path:None,kind:AgentMessageKind::Agent}); } } else { self.agent_messages.push(AgentMessage{text,path:None,kind:AgentMessageKind::Agent}); } self.agent_panel_visible=true; Ok(json!({"received":true})) }
            "agent.activity" => { let text=params.get("text").and_then(Value::as_str).ok_or("missing text")?.to_owned(); let path=params.get("path").and_then(Value::as_str).map(|path|self.explorer.root().join(path)); self.agent_messages.push(AgentMessage{text,path,kind:AgentMessageKind::Activity}); self.agent_panel_visible=true; Ok(json!({"received":true})) }
            "command.run" => { let id=params.get("id").and_then(Value::as_str).ok_or("missing command id")?; let command=Command::from_id(id).ok_or("unknown command")?; self.execute_command(command).map_err(|error|error.to_string())?; Ok(json!({"executed":id})) }
            "file.create" => { let path=self.safe_agent_new_path(params)?; fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error|error.to_string())?; self.explorer.refresh(); Ok(json!({"path":path})) }
            "file.delete" => { let path=self.safe_agent_path(params)?; if self.path_is_open(&path) { return Err("close the file before deleting it".into()); } if path.is_dir() { fs::remove_dir(&path) } else { fs::remove_file(&path) }.map_err(|error|error.to_string())?; self.explorer.refresh(); Ok(json!({"deleted":path})) }
            "edit.apply_batch" => self.apply_agent_batch(params),
            _ => Err("method not found".into()),
        }
    }

    fn agent_buffer(&self, params: &Value) -> Result<&Buffer, String> {
        let id = required_u64(params, "buffer_id")?;
        self.buffers
            .iter()
            .find(|buffer| buffer.id() == id)
            .ok_or_else(|| "unknown buffer".into())
    }

    fn apply_agent_text_operation(&mut self, params: &Value) -> Result<Value, String> {
        let path = self.safe_agent_path(params)?;
        if !self.path_is_open(&path) {
            self.open_path(path.clone());
        }
        let index = self
            .buffers
            .iter()
            .position(|buffer| buffer.path().is_some_and(|open| same_path(open, &path)))
            .ok_or("could not open path")?;
        let expected_revision = required_u64(params, "revision")?;
        if self.buffers[index].revision() != expected_revision {
            return Err(format!(
                "stale editor state: expected revision {expected_revision}, current revision {}",
                self.buffers[index].revision()
            ));
        }
        let operation = params
            .get("operation")
            .and_then(Value::as_str)
            .ok_or("missing operation")?;
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .ok_or("missing text")?
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let (start, end, start_line, start_column) = match operation {
            "insert" => {
                let (offset, line, column) = agent_position(&self.buffers[index], params)?;
                (offset, offset, line, column)
            }
            "replace_selection" => {
                let selection = params.get("selection").ok_or("missing selection")?;
                let start_value = selection.get("start").ok_or("missing selection.start")?;
                let end_value = selection.get("end").ok_or("missing selection.end")?;
                let (start, start_line, start_column) =
                    agent_position(&self.buffers[index], start_value)?;
                let (end, _, _) = agent_position(&self.buffers[index], end_value)?;
                if start > end {
                    return Err("selection start is after selection end".into());
                }
                (start, end, start_line, start_column)
            }
            "append" => {
                let offset = self.buffers[index].len_chars();
                let (line, column) = agent_offset_position(&self.buffers[index], offset);
                (offset, offset, line, column)
            }
            _ => return Err("operation must be insert, replace_selection, or append".into()),
        };
        let inserted_chars = text.chars().count();
        let revision = self.buffers[index]
            .apply_agent_edit(expected_revision, start, end, &text)
            .map_err(str::to_owned)?;
        let (end_line, end_column) = inserted_end_position(start_line, start_column, &text);
        let id = self.buffers[index].id();
        let tracked = self.agent_modified.entry(id).or_insert((0, revision));
        tracked.0 += 1;
        tracked.1 = revision;
        self.active = index;
        self.update_active_split_buffer();
        self.changed();
        self.ensure_visible();
        Ok(json!({
            "buffer_id":id,
            "revision":revision,
            "range":{
                "start":{"line":start_line,"column":start_column},
                "end":{"line":end_line,"column":end_column},
                "start_char":start,
                "end_char":start + inserted_chars
            }
        }))
    }
    fn safe_agent_path(&self, params: &Value) -> Result<PathBuf, String> {
        let raw = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or("missing path")?;
        let path = self
            .explorer
            .root()
            .join(raw)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let root = self
            .explorer
            .root()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        path.starts_with(&root)
            .then_some(path)
            .ok_or_else(|| "path escapes workspace".into())
    }
    fn safe_agent_new_path(&self, params: &Value) -> Result<PathBuf, String> {
        let raw = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or("missing path")?;
        let path = self.explorer.root().join(raw);
        let parent = path
            .parent()
            .ok_or("invalid path")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let root = self
            .explorer
            .root()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        parent
            .starts_with(&root)
            .then_some(path)
            .ok_or_else(|| "path escapes workspace".into())
    }

    fn apply_agent_batch(&mut self, params: &Value) -> Result<Value, String> {
        let edits = params
            .get("edits")
            .and_then(Value::as_array)
            .ok_or("missing edits")?;
        let mut grouped = HashMap::<u64, (u64, Vec<(usize, usize, String)>)>::new();
        for edit in edits {
            let id = required_u64(edit, "buffer_id")?;
            let revision = required_u64(edit, "revision")?;
            let start = required_u64(edit, "start")? as usize;
            let end = required_u64(edit, "end")? as usize;
            let text = edit
                .get("text")
                .and_then(Value::as_str)
                .ok_or("missing text")?
                .to_owned();
            let entry = grouped.entry(id).or_insert_with(|| (revision, Vec::new()));
            if entry.0 != revision {
                return Err("batch contains conflicting revisions".into());
            }
            entry.1.push((start, end, text));
        }
        for (id, (revision, edits)) in &grouped {
            let buffer = self
                .buffers
                .iter()
                .find(|buffer| buffer.id() == *id)
                .ok_or("unknown buffer")?;
            if buffer.revision() != *revision {
                return Err("stale buffer revision".into());
            }
            if buffer.is_read_only() {
                return Err("buffer is read-only".into());
            }
            if edits
                .iter()
                .any(|(start, end, _)| start > end || *end > buffer.len_chars())
            {
                return Err("invalid edit range".into());
            }
        }
        let mut results = Vec::new();
        for (id, (revision, edits)) in grouped {
            let buffer = self
                .buffers
                .iter_mut()
                .find(|buffer| buffer.id() == id)
                .ok_or("unknown buffer")?;
            let revision = buffer
                .apply_agent_edits(revision, &edits)
                .map_err(str::to_owned)?;
            results.push(json!({"buffer_id":id,"revision":revision}));
            let tracked = self.agent_modified.entry(id).or_insert((0, revision));
            tracked.0 += 1;
            tracked.1 = revision;
        }
        self.changed();
        Ok(Value::Array(results))
    }

    fn agent_panel_key(&mut self, key: KeyEvent) -> Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if self.agent_approval.is_some() {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.answer_agent_approval(true)
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.answer_agent_approval(false)
                }
                _ => {}
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Tab => self.agent_panel_focused = false,
            KeyCode::Char('g') | KeyCode::Char('G') if ctrl => self.agent_panel_focused = false,
            KeyCode::PageUp => self.agent_scroll = self.agent_scroll.saturating_add(8),
            KeyCode::PageDown => self.agent_scroll = self.agent_scroll.saturating_sub(8),
            KeyCode::Home if ctrl => self.agent_scroll = usize::MAX,
            KeyCode::End if ctrl => self.agent_scroll = 0,
            KeyCode::Esc if matches!(self.agent_backend_status, BuiltinAgentStatus::Working) => {
                self.stop_agent()
            }
            KeyCode::Esc => self.agent_panel_focused = false,
            KeyCode::Char('l') if ctrl => self.new_agent_conversation(),
            KeyCode::Char('r') if ctrl => self.retry_agent(),
            KeyCode::Char('k') if ctrl => self.clear_agent_chat(),
            KeyCode::Char('a') if ctrl => {
                self.agent_input_anchor = Some(0);
                self.agent_input_cursor = self.agent_input.chars().count();
            }
            KeyCode::Char('c') if ctrl => {
                if let Some(text) = self.selected_agent_input() {
                    self.copy_to_terminal_clipboard(&text)?;
                    self.clipboard = Some(text);
                    self.message = "Copied agent prompt selection".into();
                } else if let Some(text) = self.selected_agent_transcript() {
                    self.copy_to_terminal_clipboard(&text)?;
                    self.clipboard = Some(text);
                    self.message = "Copied agent conversation selection".into();
                }
            }
            KeyCode::Char('x') if ctrl => {
                if let Some(text) = self.selected_agent_input() {
                    self.copy_to_terminal_clipboard(&text)?;
                    self.clipboard = Some(text);
                    self.delete_agent_input_selection();
                }
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(text) = self.clipboard.clone() {
                    self.insert_agent_input(&text);
                } else {
                    self.message =
                        "TTED clipboard is empty; use your terminal's paste shortcut".into();
                }
            }
            KeyCode::Left => self.move_agent_input_cursor(-1, shift),
            KeyCode::Right => self.move_agent_input_cursor(1, shift),
            KeyCode::Home => self.set_agent_input_cursor(0, shift),
            KeyCode::End => {
                let end = self.agent_input.chars().count();
                self.set_agent_input_cursor(end, shift);
            }
            KeyCode::Enter if shift => self.insert_agent_input("\n"),
            KeyCode::Backspace => self.backspace_agent_input(),
            KeyCode::Enter => {
                if matches!(self.agent_backend_status, BuiltinAgentStatus::SignIn) {
                    if let Some(backend) = &self.agent_backend {
                        backend.send(BackendCommand::Login);
                        self.agent_backend_status = BuiltinAgentStatus::Starting;
                    }
                    return Ok(false);
                }
                let prompt = std::mem::take(&mut self.agent_input);
                self.agent_input_cursor = 0;
                self.agent_input_anchor = None;
                if !prompt.trim().is_empty() {
                    self.enqueue_agent_prompt(prompt);
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_agent_input(&character.to_string())
            }
            _ => {}
        }
        Ok(false)
    }

    fn agent_input_selection(&self) -> Option<(usize, usize)> {
        self.agent_input_anchor
            .filter(|anchor| *anchor != self.agent_input_cursor)
            .map(|anchor| {
                if anchor < self.agent_input_cursor {
                    (anchor, self.agent_input_cursor)
                } else {
                    (self.agent_input_cursor, anchor)
                }
            })
    }

    fn selected_agent_input(&self) -> Option<String> {
        let (start, end) = self.agent_input_selection()?;
        Some(
            self.agent_input
                .chars()
                .skip(start)
                .take(end - start)
                .collect(),
        )
    }

    fn agent_transcript_selection(&self) -> Option<(usize, usize)> {
        let (anchor, cursor) = (self.agent_transcript_anchor?, self.agent_transcript_cursor?);
        (anchor != cursor).then_some(if anchor < cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        })
    }

    fn selected_agent_transcript(&self) -> Option<String> {
        let (start, end) = self.agent_transcript_selection()?;
        Some(
            self.agent_transcript_text
                .chars()
                .skip(start)
                .take(end - start)
                .collect(),
        )
    }

    fn delete_agent_input_selection(&mut self) -> bool {
        let Some((start, end)) = self.agent_input_selection() else {
            return false;
        };
        self.agent_input = self
            .agent_input
            .chars()
            .take(start)
            .chain(self.agent_input.chars().skip(end))
            .collect();
        self.agent_input_cursor = start;
        self.agent_input_anchor = None;
        true
    }

    fn insert_agent_input(&mut self, text: &str) {
        self.delete_agent_input_selection();
        let cursor = self.agent_input_cursor;
        self.agent_input = self
            .agent_input
            .chars()
            .take(cursor)
            .chain(text.chars())
            .chain(self.agent_input.chars().skip(cursor))
            .collect();
        self.agent_input_cursor += text.chars().count();
        self.agent_input_anchor = None;
    }

    fn backspace_agent_input(&mut self) {
        if self.delete_agent_input_selection() || self.agent_input_cursor == 0 {
            return;
        }
        self.agent_input_anchor = Some(self.agent_input_cursor - 1);
        self.delete_agent_input_selection();
    }

    fn set_agent_input_cursor(&mut self, cursor: usize, select: bool) {
        if select {
            self.agent_input_anchor
                .get_or_insert(self.agent_input_cursor);
        } else {
            self.agent_input_anchor = None;
        }
        self.agent_input_cursor = cursor.min(self.agent_input.chars().count());
    }

    fn move_agent_input_cursor(&mut self, delta: isize, select: bool) {
        let cursor = if delta < 0 {
            self.agent_input_cursor.saturating_sub(delta.unsigned_abs())
        } else {
            self.agent_input_cursor
                .saturating_add(delta as usize)
                .min(self.agent_input.chars().count())
        };
        self.set_agent_input_cursor(cursor, select);
    }

    fn enqueue_agent_prompt(&mut self, prompt: String) {
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.is_dirty() && buffer.path().is_none())
        {
            self.active = index;
            self.update_active_split_buffer();
            self.agent_input = prompt;
            self.agent_input_cursor = self.agent_input.chars().count();
            self.agent_input_anchor = None;
            self.agent_messages.push(AgentMessage {
                text: "! Give the untitled file a name, then send again".into(),
                path: None,
                kind: AgentMessageKind::Error,
            });
            self.agent_panel_focused = false;
            self.path_prompt = Some(String::new());
            self.message = "Name the untitled file so Codex can work with it".into();
            return;
        }
        let mut saved = Vec::new();
        for buffer in self.buffers.iter_mut().filter(|buffer| buffer.is_dirty()) {
            let name = buffer.name();
            if let Err(error) = buffer.save() {
                self.agent_input = prompt;
                self.agent_input_cursor = self.agent_input.chars().count();
                self.agent_input_anchor = None;
                self.agent_messages.push(AgentMessage {
                    text: format!("! Could not synchronize {name}: {error}"),
                    path: None,
                    kind: AgentMessageKind::Error,
                });
                self.message = format!("Could not save {name} for Codex");
                return;
            }
            if let Some(path) = buffer.path() {
                saved.push(path.to_path_buf());
            }
        }
        if !saved.is_empty() {
            self.explorer.refresh();
            self.git.request_refresh();
            if let Some(lsp) = &self.lsp {
                for path in &saved {
                    lsp.save(path.clone());
                }
            }
        }
        self.ensure_agent_backend();
        self.agent_messages.push(AgentMessage {
            text: prompt.clone(),
            path: None,
            kind: AgentMessageKind::Human,
        });
        self.agent_scroll = 0;
        self.agent_last_prompt = Some(prompt.clone());
        self.agent_disk_before = self.capture_workspace_files();
        self.agent_disk_changes.clear();
        if let Some(backend) = &self.agent_backend {
            backend.send(BackendCommand::Prompt(
                self.agent_prompt_with_context(&prompt),
            ));
        } else {
            self.agent_prompts.push_back(prompt);
        }
        self.agent_panel_visible = true;
        self.message = if saved.is_empty() {
            "Sent to Codex".into()
        } else {
            format!("Saved {} open file(s) and sent to Codex", saved.len())
        };
    }

    fn ensure_agent_backend(&mut self) {
        if self.agent_backend.is_none() && !cfg!(test) {
            self.agent_backend_status = BuiltinAgentStatus::Starting;
            self.agent_backend = Some(AgentBackend::start(self.explorer.root().to_path_buf()));
        }
    }

    fn agent_prompt_with_context(&self, prompt: &str) -> String {
        let path = self
            .current()
            .path()
            .and_then(|path| path.strip_prefix(self.explorer.root()).ok())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.current().name());
        let (line, column) = self.current().cursor_line_col();
        let selection = self.current().selected_text().unwrap_or_default();
        let selection_range = self.current().selection().map_or_else(
            || "none".into(),
            |(start, end)| {
                let start = agent_offset_position(self.current(), start);
                let end = agent_offset_position(self.current(), end);
                format!("{}:{}-{}:{}", start.0, start.1, end.0, end.1)
            },
        );
        format!(
            "{prompt}\n\nTTED context (zero-based Unicode line:column):\nCurrent file: {path}\nCursor: {line}:{column}\nSelection range: {selection_range}\nSelection text:\n{selection}"
        )
    }

    fn poll_agent_backend(&mut self) -> bool {
        let mut changed = false;
        while let Some(event) = self.agent_backend.as_ref().and_then(AgentBackend::try_recv) {
            changed = true;
            match event {
                BackendEvent::Starting => self.agent_backend_status = BuiltinAgentStatus::Starting,
                BackendEvent::Missing => self.agent_backend_status = BuiltinAgentStatus::Missing,
                BackendEvent::Ready { authenticated } => {
                    self.agent_backend_status = if authenticated {
                        BuiltinAgentStatus::Ready
                    } else {
                        BuiltinAgentStatus::SignIn
                    };
                }
                BackendEvent::LoginCode { url, code } => {
                    self.agent_backend_status = BuiltinAgentStatus::LoginCode {
                        url: url.clone(),
                        code: code.clone(),
                    };
                    self.agent_messages.push(AgentMessage {
                        text: format!("Sign in at {url} with code {code}"),
                        path: None,
                        kind: AgentMessageKind::Activity,
                    });
                }
                BackendEvent::TurnStarted => {
                    self.agent_backend_status = BuiltinAgentStatus::Working;
                    self.agent_turn_diff.clear();
                    self.agent_messages.push(AgentMessage {
                        text: String::new(),
                        path: None,
                        kind: AgentMessageKind::Agent,
                    });
                    self.agent_stream_message = Some(self.agent_messages.len() - 1);
                }
                BackendEvent::Delta(delta) => {
                    if let Some(index) = self.agent_stream_message {
                        if let Some(message) = self.agent_messages.get_mut(index) {
                            message.text.push_str(&delta);
                        }
                    }
                }
                BackendEvent::Activity(text) => self.agent_messages.push(AgentMessage {
                    text,
                    path: None,
                    kind: AgentMessageKind::Activity,
                }),
                BackendEvent::Approval { id, detail } => {
                    self.agent_approval = Some(AgentApproval { id, detail });
                    self.agent_panel_focused = true;
                    self.message = "Codex is waiting for your approval".into();
                }
                BackendEvent::Diff(diff) => self.agent_turn_diff = diff,
                BackendEvent::Completed(status) => {
                    self.agent_approval = None;
                    self.agent_stream_message = None;
                    self.agent_backend_status = BuiltinAgentStatus::Ready;
                    self.agent_messages.push(AgentMessage {
                        text: format!("✓ Codex {status}"),
                        path: None,
                        kind: AgentMessageKind::Activity,
                    });
                    self.explorer.refresh();
                    self.git.request_refresh();
                    changed |= self.check_external_files();
                    self.finish_agent_disk_changes();
                }
                BackendEvent::Error(error) => {
                    self.agent_approval = None;
                    self.agent_stream_message = None;
                    self.agent_backend_status = BuiltinAgentStatus::Error(error.clone());
                    self.agent_messages.push(AgentMessage {
                        text: error,
                        path: None,
                        kind: AgentMessageKind::Error,
                    });
                }
            }
        }
        changed
    }

    fn stop_agent(&mut self) {
        if let Some(backend) = &self.agent_backend {
            backend.send(BackendCommand::Interrupt);
            self.message = "Stopping Codex…".into();
        }
    }

    fn answer_agent_approval(&mut self, accept: bool) {
        let Some(approval) = self.agent_approval.take() else {
            return;
        };
        if let Some(backend) = &self.agent_backend {
            backend.send(BackendCommand::Approval {
                id: approval.id,
                accept,
            });
        }
        self.message = if accept {
            "Allowed this Codex action"
        } else {
            "Declined this Codex action"
        }
        .into();
    }

    fn new_agent_conversation(&mut self) {
        if let Some(backend) = &self.agent_backend {
            backend.send(BackendCommand::NewConversation);
        }
        self.agent_messages.clear();
        self.agent_scroll = 0;
        self.agent_turn_diff.clear();
        self.agent_stream_message = None;
        self.message = "Started a new Codex conversation".into();
    }

    fn retry_agent(&mut self) {
        if let Some(prompt) = self.agent_last_prompt.clone() {
            self.enqueue_agent_prompt(prompt);
        } else {
            self.message = "There is no prompt to retry".into();
        }
    }

    fn clear_agent_chat(&mut self) {
        self.agent_messages.clear();
        self.agent_scroll = 0;
        self.message = "Cleared Agent chat".into();
    }

    fn open_agent_diff(&mut self) {
        let text = if self.agent_turn_diff.is_empty() {
            self.git.snapshot().workspace_diff()
        } else {
            self.agent_turn_diff.clone()
        };
        self.open_read_only("Agent Changes.diff", text);
    }

    fn capture_workspace_files(&self) -> HashMap<PathBuf, Vec<u8>> {
        let root = self.explorer.root();
        let mut total = 0usize;
        collect_workspace_files(root, 20_000)
            .into_iter()
            .filter_map(|relative| {
                let path = root.join(relative);
                let data = fs::read(&path).ok()?;
                if data.len() > 2_000_000 || total.saturating_add(data.len()) > 20_000_000 {
                    return None;
                }
                total += data.len();
                Some((path, data))
            })
            .collect()
    }

    fn finish_agent_disk_changes(&mut self) {
        let after = self.capture_workspace_files();
        let mut paths = self
            .agent_disk_before
            .keys()
            .chain(after.keys())
            .cloned()
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        self.agent_disk_changes = paths
            .into_iter()
            .filter_map(|path| {
                let before = self.agent_disk_before.get(&path).cloned();
                let after = after.get(&path).cloned();
                (before != after).then_some((path, AgentDiskChange { before, after }))
            })
            .collect();
    }

    fn enqueue_context_prompt(&mut self, instruction: &str) {
        let path = self
            .current()
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.current().name());
        let (line, column) = self.current().cursor_line_col();
        let selection = self.current().selected_text().unwrap_or_default();
        self.enqueue_agent_prompt(format!(
            "{instruction}\nFile: {path}\nCursor: {}:{}\nSelection:\n{selection}",
            line + 1,
            column + 1
        ));
    }

    fn accept_agent_changes(&mut self) {
        let ids = self.agent_modified.keys().copied().collect::<Vec<_>>();
        let mut saved = 0;
        for id in ids {
            if let Some(buffer) = self.buffers.iter_mut().find(|buffer| buffer.id() == id) {
                if buffer.path().is_some() && buffer.save().is_ok() {
                    saved += 1;
                    self.agent_modified.remove(&id);
                }
            }
        }
        let disk = self.agent_disk_changes.len();
        self.agent_disk_changes.clear();
        self.agent_disk_before.clear();
        self.git.request_refresh();
        self.message = if self.agent_modified.is_empty() {
            format!("Accepted {disk} Codex file(s) and saved {saved} API-edited file(s)")
        } else {
            format!(
                "Saved {saved} file(s); unsaved agent changes remain in {} buffer(s)",
                self.agent_modified.len()
            )
        };
    }

    fn revert_agent_changes(&mut self) {
        let changes = std::mem::take(&mut self.agent_modified);
        let mut skipped = 0;
        for (id, (count, revision)) in changes {
            if let Some(buffer) = self.buffers.iter_mut().find(|buffer| buffer.id() == id) {
                if buffer.revision() != revision {
                    self.agent_modified.insert(id, (count, revision));
                    skipped += 1;
                    continue;
                }
                for _ in 0..count {
                    buffer.undo();
                }
            }
        }
        let disk_changes = std::mem::take(&mut self.agent_disk_changes);
        for (path, change) in disk_changes {
            let current = fs::read(&path).ok();
            if current != change.after {
                self.agent_disk_changes.insert(path, change);
                skipped += 1;
                continue;
            }
            let result = match change.before {
                Some(data) => fs::write(&path, data),
                None => fs::remove_file(&path),
            };
            if result.is_err() {
                skipped += 1;
            }
        }
        self.explorer.refresh();
        self.git.request_refresh();
        let _ = self.check_external_files();
        self.message = if skipped == 0 {
            "Reverted tracked agent changes".into()
        } else {
            format!("Revert skipped {skipped} buffer(s) changed after the agent edit")
        };
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

    fn toggle_markdown_task(&mut self, rendered_line: usize, rendered_column: usize) {
        if self.current().is_read_only() {
            self.message = "This Markdown view is read-only".into();
            return;
        }
        let document = crate::markdown::render_document(&self.current().text());
        let Some(task) = document.tasks.into_iter().find(|task| {
            task.rendered_line == rendered_line
                && rendered_column >= task.rendered_column
                && rendered_column < task.rendered_column + 3
        }) else {
            return;
        };
        let revision = self.current().revision();
        let replacement = if task.checked { " " } else { "x" };
        if self
            .current_mut()
            .apply_agent_edit(
                revision,
                task.source_marker_char,
                task.source_marker_char + 1,
                replacement,
            )
            .is_ok()
        {
            self.changed();
            self.message = if task.checked {
                "Markdown task unchecked"
            } else {
                "Markdown task checked"
            }
            .into();
        }
    }

    fn document_max_top(&self) -> usize {
        self.current()
            .len_lines()
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
        self.problems_area = None;
        self.agent_area = None;
        self.agent_hits.clear();
        let mut workspace_area = areas[1];
        if self.problems_visible && !self.focus_mode && areas[1].height >= 10 {
            let sections =
                Layout::vertical([Constraint::Min(1), Constraint::Length(8)]).split(areas[1]);
            workspace_area = sections[0];
            self.problems_area = Some(sections[1]);
        }
        if self.agent_panel_visible && !self.focus_mode && workspace_area.width >= 60 {
            let columns =
                Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .split(workspace_area);
            workspace_area = columns[0];
            self.agent_area = Some(columns[1]);
        }
        if self.focus_mode {
            self.body = frame.area();
        } else if self.sidebar_visible && workspace_area.width >= 40 {
            let sidebar_width = workspace_area.width.saturating_div(3).clamp(18, 32);
            let columns =
                Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)])
                    .split(workspace_area);
            self.sidebar_area = Some(columns[0]);
            self.body = columns[1];
            self.render_sidebar(frame, columns[0]);
        } else {
            self.body = workspace_area;
        }
        self.secondary_area = None;
        if let Some(split) = &self.split {
            let panes = if split.direction == SplitDirection::Right {
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(self.body)
            } else {
                Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(self.body)
            };
            self.body = panes[split.active];
            self.secondary_area = Some(panes[1 - split.active]);
        }
        self.gutter_width = if self.markdown_reading[self.active] {
            0
        } else if !self.config.editor.line_numbers {
            2
        } else {
            ((self.top_line + usize::from(self.body.height)).min(self.current().len_lines()))
                .max(1)
                .to_string()
                .len() as u16
                + 3
        };
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
            self.render_secondary_pane(frame);
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
            self.render_problems(frame);
            self.render_agent_panel(frame);
            self.render_lsp_popups(frame);
            self.render_git_prompts(frame);
            self.render_keybindings_menu(frame);
            return;
        }

        let selection = self.current().selection();
        let cursor = self.current().cursor();
        let current_path = self.current().path().map(PathBuf::from);
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
            let diagnostic = current_path
                .as_deref()
                .and_then(|path| self.lsp.as_ref()?.diagnostic_at(path, line_idx));
            let marker = diagnostic
                .map(|item| if item.severity == 1 { 'E' } else { 'W' })
                .or_else(|| {
                    current_path
                        .as_deref()
                        .and_then(|path| self.git.snapshot().line_decoration(path, line_idx + 1))
                });
            let marker_style = Style::default().bg(theme::MANTLE).fg(match marker {
                Some('E') => theme::RED,
                Some('W') => theme::PEACH,
                Some('A') => theme::GREEN,
                Some('D') => theme::RED,
                Some('M') => theme::PEACH,
                _ => theme::MANTLE,
            });
            let mut spans = vec![Span::styled(
                format!("{} ", marker.unwrap_or(' ')),
                marker_style,
            )];
            if self.config.editor.line_numbers {
                spans.push(Span::styled(
                    format!(
                        "{:>width$} ",
                        line_idx + 1,
                        width = usize::from(self.gutter_width - 3)
                    ),
                    Style::default().fg(theme::OVERLAY0).bg(theme::MANTLE),
                ));
            }
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
        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
            .block(Block::default().borders(Borders::NONE));
        frame.render_widget(
            if self.config.editor.word_wrap {
                paragraph.wrap(Wrap { trim: false })
            } else {
                paragraph
            },
            self.body,
        );
        self.render_secondary_pane(frame);

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
        let git = self.git_status_text();
        let lsp = self.lsp.as_ref().map_or(String::new(), |service| {
            format!("   LSP {}", service.status())
        });
        let agent = self
            .agent
            .as_ref()
            .map_or(String::new(), |_| "   Agent API".into());
        let status = format!(
            "{left}   Ln {}, Col {}{git}{lsp}{agent}   F1 help  Ctrl+G agent  Ctrl+S save  Ctrl+Q quit",
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
        self.render_problems(frame);
        self.render_agent_panel(frame);
        self.render_lsp_popups(frame);
        self.render_git_prompts(frame);
        self.render_keybindings_menu(frame);
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
            "  Ctrl+A                                Select all",
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
            "  F2 / Ctrl+Shift+P                     Command Palette",
            "  F3                                    Change keybindings",
            "  Ctrl+W                                Close tab",
            "  Ctrl+E                                Toggle/focus file explorer",
            "  Explorer: arrows, Enter, Esc/Tab      Navigate/open/return",
            "  Explorer: N / Shift+N / R / D         New file/dir, rename/delete",
            "  Ctrl+Shift+M / F6                     Markdown reading view",
            "  Ctrl+G / F9                           Toggle Agent chat",
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

    fn render_problems(&self, frame: &mut Frame) {
        let Some(area) = self.problems_area else {
            return;
        };
        let items = self.current_diagnostics();
        let visible = usize::from(area.height.saturating_sub(2));
        let start = self
            .problems_selected
            .saturating_sub(visible.saturating_sub(1));
        let lines = items
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(index, item)| {
                let severity = if item.severity == 1 {
                    "error"
                } else {
                    "warning"
                };
                let label = format!(
                    " {severity} {}:{}:{}  {}",
                    item.path.display(),
                    item.line + 1,
                    item.column + 1,
                    item.message
                );
                Line::styled(
                    label,
                    if index == self.problems_selected {
                        Style::default().bg(theme::SURFACE1).fg(theme::TEXT)
                    } else if item.severity == 1 {
                        Style::default().fg(theme::RED)
                    } else {
                        Style::default().fg(theme::PEACH)
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::MANTLE))
                .block(
                    Block::bordered()
                        .title(format!(" Problems ({}) — F8 next ", items.len()))
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            area,
        );
    }

    fn render_agent_panel(&mut self, frame: &mut Frame) {
        let Some(area) = self.agent_area else {
            return;
        };
        let input_width = usize::from(area.width.saturating_sub(4).max(1));
        let input_lines = wrap_agent_text(&self.agent_input, input_width).len().max(1);
        let input_height = (input_lines as u16).saturating_add(2).clamp(3, 8);
        let sections = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(input_height),
            Constraint::Length(2),
        ])
        .split(area);
        self.agent_input_area = Some(sections[1]);
        let visible = usize::from(sections[0].height.saturating_sub(2));
        let content_width = usize::from(sections[0].width.saturating_sub(10).max(1));
        let mut transcript = Vec::<(&str, Color, Color, String, Option<PathBuf>, usize)>::new();
        self.agent_transcript_text.clear();
        for message in &self.agent_messages {
            let (label, color, background) = match message.kind {
                AgentMessageKind::Human => ("YOU   ", theme::BLUE, theme::SURFACE0),
                AgentMessageKind::Agent => ("CODEX ", theme::GREEN, theme::MANTLE),
                AgentMessageKind::Activity => ("  ·   ", theme::OVERLAY0, theme::MANTLE),
                AgentMessageKind::Error => ("  !   ", theme::RED, theme::MANTLE),
            };
            for (index, text) in wrap_agent_text(&message.text, content_width)
                .into_iter()
                .enumerate()
            {
                let offset = self.agent_transcript_text.chars().count();
                self.agent_transcript_text.push_str(&text);
                self.agent_transcript_text.push('\n');
                transcript.push((
                    if index == 0 { label } else { "      " },
                    color,
                    background,
                    text,
                    message.path.clone(),
                    offset,
                ));
            }
        }
        let max_scroll = transcript.len().saturating_sub(visible);
        self.agent_scroll = self.agent_scroll.min(max_scroll);
        let start = transcript
            .len()
            .saturating_sub(visible)
            .saturating_sub(self.agent_scroll);
        self.agent_transcript_hits.clear();
        let transcript_selection = self.agent_transcript_selection();
        let lines = transcript
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
            .map(
                |(visible_offset, (label, color, background, text, path, text_offset))| {
                    let row = sections[0].y + 1 + visible_offset as u16;
                    if let Some(path) = path {
                        self.agent_hits.push((row, path.clone()));
                    }
                    self.agent_transcript_hits
                        .push((row, *text_offset, text.clone()));
                    let mut spans = vec![Span::styled(
                        *label,
                        Style::default().fg(*color).add_modifier(Modifier::BOLD),
                    )];
                    for (character_offset, character) in text.chars().enumerate() {
                        let position = *text_offset + character_offset;
                        let selected =
                            transcript_selection.is_some_and(|(selection_start, selection_end)| {
                                position >= selection_start && position < selection_end
                            });
                        spans.push(Span::styled(
                            character.to_string(),
                            if selected {
                                Style::default().bg(theme::BLUE).fg(theme::BASE)
                            } else {
                                Style::default().fg(theme::TEXT)
                            },
                        ));
                    }
                    Line::from(spans).style(Style::default().bg(*background))
                },
            )
            .collect::<Vec<_>>();
        let status = match &self.agent_backend_status {
            BuiltinAgentStatus::Idle => "Agent",
            BuiltinAgentStatus::Starting => "Codex — connecting…",
            BuiltinAgentStatus::SignIn => "Codex — press Enter to sign in",
            BuiltinAgentStatus::Ready if self.agent_scroll > 0 => "Codex — history (End: latest)",
            BuiltinAgentStatus::Ready => "Codex — ready",
            BuiltinAgentStatus::Working => "Codex — working (Esc stops)",
            BuiltinAgentStatus::LoginCode { .. } => "Codex — waiting for sign-in",
            BuiltinAgentStatus::Missing => "Codex not installed — install the Codex CLI",
            BuiltinAgentStatus::Error(_) => "Codex — needs attention",
        };
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::MANTLE))
                .block(
                    Block::bordered()
                        .title(format!(" {status} "))
                        .border_style(Style::default().fg(theme::MAUVE)),
                ),
            sections[0],
        );
        let selection = self.agent_input_selection();
        let mut input_lines = Vec::<Line<'static>>::new();
        let mut input_spans = vec![Span::styled("> ", Style::default().fg(theme::MAUVE))];
        let mut input_column = 2;
        for (index, character) in self.agent_input.chars().enumerate() {
            if self.agent_panel_focused && index == self.agent_input_cursor {
                if input_column >= input_width {
                    input_lines.push(Line::from(std::mem::take(&mut input_spans)));
                    input_column = 0;
                }
                input_spans.push(Span::styled("▏", Style::default().fg(theme::GREEN)));
                input_column += 1;
            }
            if character == '\n' {
                input_lines.push(Line::from(std::mem::take(&mut input_spans)));
                input_column = 0;
                continue;
            }
            let character_text = character.to_string();
            let character_width = UnicodeWidthStr::width(character_text.as_str());
            if input_column > 0 && input_column + character_width > input_width {
                input_lines.push(Line::from(std::mem::take(&mut input_spans)));
                input_column = 0;
            }
            let selected = selection.is_some_and(|(start, end)| index >= start && index < end);
            input_spans.push(Span::styled(
                character_text,
                if selected {
                    Style::default().bg(theme::BLUE).fg(theme::BASE)
                } else {
                    Style::default().fg(theme::TEXT)
                },
            ));
            input_column += character_width;
        }
        if self.agent_panel_focused && self.agent_input_cursor == self.agent_input.chars().count() {
            if input_column >= input_width {
                input_lines.push(Line::from(std::mem::take(&mut input_spans)));
            }
            input_spans.push(Span::styled("▏", Style::default().fg(theme::GREEN)));
        }
        input_lines.push(Line::from(input_spans));
        let total_input_lines = input_lines.len();
        let visible_input_lines = usize::from(sections[1].height.saturating_sub(2).max(1));
        let input_start = total_input_lines.saturating_sub(visible_input_lines);
        frame.render_widget(
            Paragraph::new(
                input_lines
                    .into_iter()
                    .skip(input_start)
                    .collect::<Vec<_>>(),
            )
            .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
            .block(
                Block::bordered()
                    .title(" Prompt — Enter send · Shift+Enter newline · Tab document ")
                    .border_style(Style::default().fg(if self.agent_panel_focused {
                        theme::GREEN
                    } else {
                        theme::SURFACE1
                    })),
            ),
            sections[1],
        );
        frame.render_widget(
            Paragraph::new("[Stop] [Retry] [New] [Clear]\n[Diff] [Accept] [Revert]")
                .style(Style::default().bg(theme::SURFACE0).fg(theme::SUBTEXT0)),
            sections[2],
        );
        let setup = match &self.agent_backend_status {
            BuiltinAgentStatus::SignIn => {
                Some("Codex is installed\n\n[ Sign in with ChatGPT ]\n\nClick or press Enter")
            }
            BuiltinAgentStatus::Missing => {
                Some("Codex is not installed\n\n[ Show install command ]\n\nClick for simple setup")
            }
            BuiltinAgentStatus::LoginCode { url, code } => {
                let text =
                    format!("Finish signing in\n\n{url}\nCode: {code}\n\nClick to copy code");
                let popup = centered_rect(area, area.width.saturating_sub(4).min(48), 9);
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Paragraph::new(text)
                        .alignment(ratatui::layout::Alignment::Center)
                        .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                        .block(
                            Block::bordered()
                                .title(" Connect Codex ")
                                .border_style(Style::default().fg(theme::GREEN)),
                        ),
                    popup,
                );
                None
            }
            BuiltinAgentStatus::Error(error) => {
                let text = format!("Codex needs attention\n\n{error}\n\nRetry the Agent panel");
                let popup = centered_rect(area, area.width.saturating_sub(4).min(48), 8);
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Paragraph::new(text)
                        .alignment(ratatui::layout::Alignment::Center)
                        .style(Style::default().bg(theme::BASE).fg(theme::RED))
                        .block(Block::bordered().title(" Agent error ")),
                    popup,
                );
                None
            }
            _ => None,
        };
        if let Some(text) = setup {
            let popup = centered_rect(area, area.width.saturating_sub(4).min(48), 8);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text)
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(" Connect an agent ")
                            .border_style(Style::default().fg(theme::MAUVE)),
                    ),
                popup,
            );
        }
        if let Some(approval) = &self.agent_approval {
            let text = format!(
                "Codex wants permission to:\n\n{}\n\n[ Allow ]          [ Decline ]\nEnter/Y allows · Esc/N declines",
                approval.detail
            );
            let popup = centered_rect(area, area.width.saturating_sub(4).min(56), 10);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text)
                    .alignment(ratatui::layout::Alignment::Center)
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(" Permission needed ")
                            .border_style(Style::default().fg(theme::PEACH)),
                    ),
                popup,
            );
        }
    }

    fn render_secondary_pane(&self, frame: &mut Frame) {
        let (Some(split), Some(area)) = (&self.split, self.secondary_area) else {
            return;
        };
        let index = split.buffers[1 - split.active].min(self.buffers.len().saturating_sub(1));
        let buffer = &self.buffers[index];
        let height = usize::from(area.height.saturating_sub(2));
        let lines = (0..buffer.len_lines())
            .take(height)
            .map(|line| {
                Line::from(vec![
                    Span::styled(
                        format!("{:>4} ", line + 1),
                        Style::default().fg(theme::OVERLAY0),
                    ),
                    Span::raw(buffer.line(line).trim_end_matches(['\r', '\n']).to_owned()),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                .block(
                    Block::bordered()
                        .title(format!(" {} — click to focus ", buffer.name()))
                        .border_style(Style::default().fg(theme::SURFACE1)),
                ),
            area,
        );
    }

    fn render_lsp_popups(&self, frame: &mut Frame) {
        let area = frame.area();
        if let Some(prompt) = &self.lsp_prompt {
            let width = area.width.clamp(1, 64);
            let height = area.height.clamp(1, 5);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 3,
                width,
                height,
            );
            let title = match prompt.kind {
                LspPromptKind::Rename => " Rename symbol ",
                LspPromptKind::WorkspaceSymbols => " Workspace symbols ",
            };
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(format!("{}_\n\nEnter submit   Esc cancel", prompt.input))
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(title)
                            .border_style(Style::default().fg(theme::MAUVE)),
                    ),
                popup,
            );
        } else if let Some(text) = &self.hover_popup {
            let width = area.width.clamp(1, 72);
            let height = area.height.clamp(1, 14);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 3,
                width,
                height,
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text.as_str())
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(" Hover — any key closes ")
                            .border_style(Style::default().fg(theme::BLUE)),
                    ),
                popup,
            );
        } else if let Some(items) = &self.completions {
            let width = area.width.clamp(1, 48);
            let height = area.height.clamp(1, 14);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 3,
                width,
                height,
            );
            let visible = usize::from(height.saturating_sub(2));
            let start = self
                .completion_selected
                .saturating_sub(visible.saturating_sub(1));
            let lines = items
                .iter()
                .enumerate()
                .skip(start)
                .take(visible)
                .map(|(index, item)| {
                    Line::styled(
                        format!(" {item}"),
                        if index == self.completion_selected {
                            Style::default().bg(theme::SURFACE1).fg(theme::TEXT)
                        } else {
                            Style::default().fg(theme::SUBTEXT0)
                        },
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(lines)
                    .style(Style::default().bg(theme::BASE))
                    .block(
                        Block::bordered()
                            .title(" Completion — Enter insert, Esc close ")
                            .border_style(Style::default().fg(theme::MAUVE)),
                    ),
                popup,
            );
        }
    }

    fn render_git_prompts(&self, frame: &mut Frame) {
        let area = frame.area();
        if let Some(path) = &self.git_discard_confirm {
            let width = area.width.clamp(1, 68);
            let height = area.height.clamp(1, 7);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 2,
                width,
                height,
            );
            let text = format!(
                "Discard working-tree changes in {}?\n\nThis cannot be undone by TTED.\n\nY discard   N/Esc cancel",
                path.display()
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text)
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(" Git discard changes ")
                            .border_style(Style::default().fg(theme::RED)),
                    ),
                popup,
            );
        } else if let Some(message) = &self.git_commit_prompt {
            let width = area.width.clamp(1, 72);
            let height = area.height.clamp(1, 7);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 3,
                width,
                height,
            );
            let text = format!("{message}_\n\nEnter commit staged changes   Esc cancel");
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text)
                    .style(Style::default().bg(theme::BASE).fg(theme::TEXT))
                    .block(
                        Block::bordered()
                            .title(" Git commit message ")
                            .border_style(Style::default().fg(theme::GREEN)),
                    ),
                popup,
            );
        }
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

    fn render_keybindings_menu(&mut self, frame: &mut Frame) {
        let Some(menu) = &self.keybindings_menu else {
            return;
        };
        let area = frame.area();
        let popup = centered_rect(area, area.width.min(76), area.height.min(24));
        self.keybindings_area = Some(popup);
        let visible = usize::from(popup.height.saturating_sub(5));
        let start = menu.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (index, command) in Command::ALL.iter().enumerate().skip(start).take(visible) {
            let custom = self
                .config
                .keybindings
                .iter()
                .find_map(|(key, id)| (id == command.id()).then_some(key.as_str()));
            let binding = custom.unwrap_or_else(|| default_binding(*command));
            lines.push(Line::styled(
                format!(" {:<45} {:>20}", command.title(), binding),
                if index == menu.selected {
                    Style::default().bg(theme::SURFACE1).fg(theme::MAUVE)
                } else {
                    Style::default().fg(theme::SUBTEXT0)
                },
            ));
        }
        lines.push(Line::default());
        lines.push(Line::styled(
            if menu.capturing {
                " Press the new shortcut · Esc cancels"
            } else {
                " ↑↓ select · Enter change · Delete reset · Esc close"
            },
            Style::default().fg(if menu.capturing {
                theme::PEACH
            } else {
                theme::TEXT
            }),
        ));
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme::BASE))
                .block(
                    Block::bordered()
                        .title(" Keybindings — saved for this workspace ")
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

    fn git_status_text(&self) -> String {
        let snapshot = self.git.snapshot();
        if !snapshot.is_repository() {
            return String::new();
        }
        let branch = snapshot.branch.as_deref().unwrap_or("detached");
        let state = if snapshot.is_dirty() { "*" } else { "✓" };
        format!("   Git {branch} {state}")
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
            let decoration = (!row.is_dir)
                .then(|| self.git.snapshot().decoration(&row.path))
                .flatten();
            let diagnostic_count = if row.is_dir {
                0
            } else {
                self.lsp
                    .as_ref()
                    .map_or(0, |lsp| lsp.diagnostic_count(&row.path))
            };
            let label = format!(
                "{}{} {name}{}{}",
                "  ".repeat(row.depth),
                marker,
                decoration.map_or_else(String::new, |status| format!("  {status}")),
                if diagnostic_count > 0 {
                    format!("  !{diagnostic_count}")
                } else {
                    String::new()
                }
            );
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
            } else if let Some(status) = decoration {
                Style::default().fg(match status {
                    'A' | '?' => theme::GREEN,
                    'D' => theme::RED,
                    _ => theme::PEACH,
                })
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
        let syntax = if let Some(path) = self.current().path() {
            self.syntaxes.find_syntax_for_file(path).ok().flatten()
        } else {
            self.current()
                .name()
                .rsplit_once('.')
                .and_then(|(_, extension)| self.syntaxes.find_syntax_by_extension(extension))
        };
        let Some(syntax) = syntax else {
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

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn default_binding(command: Command) -> &'static str {
    match command {
        Command::NewFile => "ctrl+n",
        Command::Save => "ctrl+s",
        Command::SaveAs => "ctrl+shift+s",
        Command::CloseFile => "ctrl+w",
        Command::QuickOpen => "ctrl+p",
        Command::ToggleExplorer => "ctrl+e",
        Command::FindReplace => "ctrl+f",
        Command::NextTab => "ctrl+tab",
        Command::PreviousTab => "ctrl+shift+tab",
        Command::ToggleMarkdownReader => "f6",
        Command::ToggleProblems => "f8",
        Command::ToggleAgentPanel => "ctrl+g / f9",
        Command::OpenKeybindings => "f3",
        Command::ShowHelp => "f1",
        Command::Quit => "ctrl+q",
        _ => "—",
    }
}

fn wrap_agent_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for source_line in text.split('\n') {
        let mut line = String::new();
        let mut line_width = 0;
        for grapheme in source_line.graphemes(true) {
            let grapheme_width = UnicodeWidthStr::width(grapheme);
            if !line.is_empty() && line_width + grapheme_width > width {
                wrapped.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push_str(grapheme);
            line_width += grapheme_width;
        }
        wrapped.push(line);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn agent_input_char_at(
    text: &str,
    width: usize,
    visible_height: usize,
    clicked_row: usize,
    clicked_column: usize,
) -> usize {
    let width = width.max(1);
    let mut rows = vec![vec![(2.min(width), 0)]];
    let mut column = 2.min(width);
    for (index, character) in text.chars().enumerate() {
        if character == '\n' {
            rows.push(vec![(0, index + 1)]);
            column = 0;
            continue;
        }
        let character_width = UnicodeWidthStr::width(character.to_string().as_str());
        if column > 0 && column + character_width > width {
            rows.push(vec![(0, index)]);
            column = 0;
        }
        column = (column + character_width).min(width);
        rows.last_mut()
            .expect("input always has a row")
            .push((column, index + 1));
    }
    let first_visible = rows.len().saturating_sub(visible_height.max(1));
    let row = rows
        .get(first_visible + clicked_row)
        .or_else(|| rows.last())
        .expect("input always has a row");
    row.iter()
        .min_by_key(|(column, _)| column.abs_diff(clicked_column))
        .map_or(0, |(_, index)| *index)
}

fn key_event_name(key: &KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_owned());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_owned());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_owned());
    }
    let key_name = match key.code {
        KeyCode::Char(character) => character.to_lowercase().collect(),
        KeyCode::F(number) => format!("f{number}"),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::Esc => "esc".into(),
        _ => return String::new(),
    };
    parts.push(key_name);
    parts.join("+")
}

fn required_u64(params: &Value, key: &str) -> Result<u64, String> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing or invalid {key}"))
}

fn agent_position(buffer: &Buffer, value: &Value) -> Result<(usize, usize, usize), String> {
    let line = required_u64(value, "line")? as usize;
    let column = required_u64(value, "column")? as usize;
    if line >= buffer.len_lines() {
        return Err(format!("line {line} is outside the file"));
    }
    let content = buffer.line(line);
    let content = content.trim_end_matches(['\n', '\r']);
    let columns = content.chars().count();
    if column > columns {
        return Err(format!(
            "column {column} is outside line {line} (maximum {columns})"
        ));
    }
    Ok((buffer.line_start_char(line) + column, line, column))
}

fn agent_offset_position(buffer: &Buffer, offset: usize) -> (usize, usize) {
    for line in (0..buffer.len_lines()).rev() {
        let start = buffer.line_start_char(line);
        if offset >= start {
            return (line, offset - start);
        }
    }
    (0, 0)
}

fn inserted_end_position(line: usize, column: usize, text: &str) -> (usize, usize) {
    let added_lines = text.chars().filter(|character| *character == '\n').count();
    if added_lines == 0 {
        (line, column + text.chars().count())
    } else {
        (
            line + added_lines,
            text.rsplit('\n').next().unwrap_or_default().chars().count(),
        )
    }
}

fn collect_workspace_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, limit: usize, files: &mut Vec<PathBuf>) {
        if files.len() >= limit {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            if files.len() >= limit {
                break;
            }
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.')
                || matches!(name.to_str(), Some("target" | "node_modules"))
            {
                continue;
            }
            if path.is_dir() {
                visit(root, &path, limit, files);
            } else if path.is_file() {
                files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, limit, &mut files);
    files.sort();
    files
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
    use serde_json::json;

    use crate::buffer::Buffer;
    use crate::explorer::Explorer;

    use super::{
        agent_input_char_at, control_letter, key_event_name, valid_entry_name, AgentMessage,
        AgentMessageKind, Command, Editor,
    };

    #[test]
    fn maps_raw_control_characters() {
        assert_eq!(control_letter('\u{3}'), Some('c'));
        assert_eq!(control_letter('\u{11}'), Some('q'));
        assert_eq!(control_letter('q'), None);
    }

    #[test]
    fn normalizes_configurable_key_names() {
        assert_eq!(
            key_event_name(&KeyEvent::new(
                KeyCode::Char('P'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            "ctrl+shift+p"
        );
        assert_eq!(
            key_event_name(&KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
            "f8"
        );
    }

    #[test]
    fn directory_argument_becomes_visible_workspace() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("example.txt"), "hello").unwrap();
        let editor = Editor::new(vec![root.path().to_path_buf()]);
        assert_eq!(editor.explorer.root(), root.path());
        assert!(editor.sidebar_visible);
        assert_eq!(editor.current().name(), "Untitled");
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
    fn ctrl_a_selects_all_document_text() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert("Hello, 🌍!\nSecond line");
        editor.current_mut().move_horizontal(-4, false);

        editor
            .key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(
            editor.current().selected_text().as_deref(),
            Some("Hello, 🌍!\nSecond line")
        );
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
    fn markdown_reading_view_checkboxes_toggle_source_and_undo() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.md");
        fs::write(&path, "- [ ] first\n- [x] second\n").unwrap();
        let mut editor = Editor::new(vec![path]);
        editor.markdown_reading[0] = true;
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();

        editor
            .handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: editor.body.x + 2,
                row: editor.body.y,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();

        assert_eq!(editor.current().text(), "- [x] first\n- [x] second\n");
        assert!(editor.current().is_dirty());
        editor.current_mut().undo();
        assert_eq!(editor.current().text(), "- [ ] first\n- [x] second\n");
    }

    #[test]
    fn mouse_wheel_scrolls_document_independently_of_cursor() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("long.txt");
        let text = (0..80)
            .map(|line| format!("Line {line}\n"))
            .collect::<String>();
        fs::write(&path, text).unwrap();
        let mut editor = Editor::new(vec![path]);
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();

        editor
            .handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: editor.body.x,
                row: editor.body.y,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();

        assert_eq!(editor.current().cursor_line_col().0, 0);
        assert_eq!(editor.top_line, 3);
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

    #[test]
    fn git_discard_refuses_unsaved_editor_content() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert("unsaved");
        editor
            .execute_command(Command::GitDiscardCurrentFile)
            .unwrap();
        assert!(editor.git_discard_confirm.is_none());
        assert!(editor.message.contains("Save or discard editor changes"));
    }

    #[test]
    fn git_commit_prompt_refuses_an_empty_message() {
        let mut editor = Editor::new(Vec::new());
        editor.execute_command(Command::GitCommit).unwrap();
        editor
            .git_commit_key(KeyEvent::from(KeyCode::Enter))
            .unwrap();
        assert!(editor.git_commit_prompt.is_some());
        assert!(editor.message.contains("cannot be empty"));
    }

    #[test]
    fn split_panes_track_independent_active_buffers() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let mut editor = Editor::new(vec![first]);
        editor.execute_command(Command::SplitRight).unwrap();
        editor.open_path(second);
        assert_eq!(editor.split.as_ref().unwrap().buffers, [1, 0]);
        editor.execute_command(Command::FocusNextSplit).unwrap();
        assert_eq!(editor.active, 0);
        editor.execute_command(Command::FocusNextSplit).unwrap();
        assert_eq!(editor.active, 1);
        editor.execute_command(Command::CloseSplit).unwrap();
        assert!(editor.split.is_none());
    }

    #[test]
    fn agent_batch_rejects_stale_input_before_any_edit() {
        let mut editor = Editor::new(Vec::new());
        let first = editor.current().id();
        editor.buffers.push(Buffer::empty());
        let second = editor.buffers[1].id();
        let result = editor.apply_agent_batch(&json!({"edits":[
            {"buffer_id":first,"revision":0,"start":0,"end":0,"text":"x"},
            {"buffer_id":second,"revision":99,"start":0,"end":0,"text":"y"}
        ]}));
        assert_eq!(result, Err("stale buffer revision".into()));
        assert_eq!(editor.buffers[0].text(), "");
        assert_eq!(editor.buffers[1].text(), "");
    }

    #[test]
    fn agent_edit_method_uses_stable_id_and_revision() {
        let mut editor = Editor::new(Vec::new());
        let id = editor.current().id();
        let result = editor
            .execute_agent_method(
                "edit.apply",
                &json!({"buffer_id":id,"revision":0,"start":0,"end":0,"text":"agent"}),
            )
            .unwrap();
        assert_eq!(result["revision"], 1);
        assert_eq!(editor.current().text(), "agent");
    }

    #[test]
    fn native_agent_edit_inserts_at_cursor_and_returns_range() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file with spaces.txt");
        fs::write(&path, "hello").unwrap();
        let mut editor = Editor::new(vec![root.path().to_path_buf(), path]);
        let result = editor
            .execute_agent_method(
                "editor.edit_text",
                &json!({"path":"file with spaces.txt","revision":0,"operation":"insert","line":0,"column":5,"text":" world"}),
            )
            .unwrap();
        assert_eq!(editor.current().text(), "hello world");
        assert_eq!(result["range"]["start"], json!({"line":0,"column":5}));
        assert_eq!(result["range"]["end"], json!({"line":0,"column":11}));
    }

    #[test]
    fn native_agent_edit_replaces_unicode_selection_with_multiline_text() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("unicode.txt");
        fs::write(&path, "a🦀z\r\nnext\r\n").unwrap();
        let mut editor = Editor::new(vec![root.path().to_path_buf(), path]);
        let result = editor
            .execute_agent_method(
                "editor.edit_text",
                &json!({
                    "path":"unicode.txt","revision":0,"operation":"replace_selection",
                    "selection":{"start":{"line":0,"column":1},"end":{"line":0,"column":2}},
                    "text":"β\r\nγ"
                }),
            )
            .unwrap();
        assert_eq!(editor.current().text(), "aβ\nγz\nnext\n");
        assert_eq!(result["range"]["end"], json!({"line":1,"column":1}));
        editor.current_mut().save().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("unicode.txt")).unwrap(),
            "aβ\r\nγz\r\nnext\r\n"
        );
    }

    #[test]
    fn native_agent_edit_appends_to_empty_file_and_rejects_stale_state() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("empty.txt");
        fs::write(&path, "").unwrap();
        let mut editor = Editor::new(vec![root.path().to_path_buf(), path]);
        editor
            .execute_agent_method(
                "editor.edit_text",
                &json!({"path":"empty.txt","revision":0,"operation":"append","text":"first\nsecond"}),
            )
            .unwrap();
        assert_eq!(editor.current().text(), "first\nsecond");
        let error = editor
            .execute_agent_method(
                "editor.edit_text",
                &json!({"path":"empty.txt","revision":0,"operation":"append","text":"lost"}),
            )
            .unwrap_err();
        assert!(error.contains("stale editor state"));
        assert_eq!(editor.current().text(), "first\nsecond");
    }

    #[test]
    fn native_agent_edit_enforces_workspace_boundary() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("workspace");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("inside.txt"), "inside").unwrap();
        fs::write(parent.path().join("outside.txt"), "outside").unwrap();
        let mut editor = Editor::new(vec![root.clone(), root.join("inside.txt")]);
        let error = editor
            .execute_agent_method(
                "editor.edit_text",
                &json!({"path":"../outside.txt","revision":0,"operation":"append","text":"bad"}),
            )
            .unwrap_err();
        assert_eq!(error, "path escapes workspace");
        assert_eq!(
            fs::read_to_string(parent.path().join("outside.txt")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn agent_prompt_queue_and_streamed_response() {
        let mut editor = Editor::new(Vec::new());
        editor.enqueue_agent_prompt("please review".into());
        let prompt = editor
            .execute_agent_method("agent.next_prompt", &json!({}))
            .unwrap();
        assert_eq!(prompt["prompt"], "please review");
        editor
            .execute_agent_method("agent.respond", &json!({"text":"Working","append":false}))
            .unwrap();
        editor
            .execute_agent_method("agent.respond", &json!({"text":"…done","append":true}))
            .unwrap();
        assert_eq!(editor.agent_messages.last().unwrap().text, "Working…done");
    }

    #[test]
    fn agent_prompt_synchronizes_dirty_named_buffers() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("live.rs");
        fs::write(&path, "old").unwrap();
        let mut editor = Editor::new(vec![path.clone()]);
        editor.current_mut().insert_typed("live edit");

        editor.enqueue_agent_prompt("work with this file".into());

        assert_eq!(fs::read_to_string(path).unwrap(), "live editold");
        assert!(!editor.current().is_dirty());
        assert_eq!(editor.agent_prompts.back().unwrap(), "work with this file");
    }

    #[test]
    fn agent_prompt_requests_a_name_only_for_dirty_untitled_buffer() {
        let mut editor = Editor::new(Vec::new());
        editor.current_mut().insert_typed("live edit");
        editor.agent_input = "help".into();

        let prompt = std::mem::take(&mut editor.agent_input);
        editor.enqueue_agent_prompt(prompt);

        assert!(editor.path_prompt.is_some());
        assert_eq!(editor.agent_input, "help");
        assert!(editor.agent_prompts.is_empty());
    }

    #[test]
    fn open_agent_panel_can_yield_focus_to_document() {
        let mut editor = Editor::new(Vec::new());
        editor.agent_panel_visible = true;
        editor.agent_panel_focused = true;
        editor
            .agent_panel_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert!(editor.agent_panel_visible);
        assert!(!editor.agent_panel_focused);

        editor
            .key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(editor.current().text(), "x");
    }

    #[test]
    fn agent_prompt_wraps_and_supports_selection_copy_and_replace() {
        let mut editor = Editor::new(Vec::new());
        editor.agent_panel_visible = true;
        editor.agent_panel_focused = true;
        editor.insert_agent_input("hello 🌍 and a prompt long enough to wrap");
        editor
            .agent_panel_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(
            editor.selected_agent_input().as_deref(),
            Some("hello 🌍 and a prompt long enough to wrap")
        );
        editor.clipboard = Some("replacement".into());
        editor
            .agent_panel_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(editor.agent_input, "replacement");

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        editor.agent_input = "word ".repeat(30);
        editor.agent_input_cursor = editor.agent_input.chars().count();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        assert!(editor.agent_input_area.expect("input area").height > 3);
    }

    #[test]
    fn agent_transcript_selection_returns_only_dragged_text() {
        let mut editor = Editor::new(Vec::new());
        editor.agent_transcript_text = "first response\nsecond response\n".into();
        editor.agent_transcript_anchor = Some(6);
        editor.agent_transcript_cursor = Some(14);
        assert_eq!(
            editor.selected_agent_transcript().as_deref(),
            Some("response")
        );
    }

    #[test]
    fn mouse_position_maps_into_wrapped_agent_prompt() {
        assert_eq!(agent_input_char_at("abcdef", 5, 3, 1, 2), 5);
        assert_eq!(agent_input_char_at("ab\ncd", 8, 3, 1, 1), 4);
    }

    #[test]
    fn agent_conversation_labels_roles_and_scrolls_independently() {
        let mut editor = Editor::new(Vec::new());
        editor.agent_panel_visible = true;
        editor.agent_panel_focused = true;
        for index in 0..20 {
            editor.agent_messages.push(AgentMessage {
                text: format!("older message {index}"),
                path: None,
                kind: AgentMessageKind::Activity,
            });
        }
        editor.agent_messages.push(AgentMessage {
            text: "my request".into(),
            path: None,
            kind: AgentMessageKind::Human,
        });
        editor.agent_messages.push(AgentMessage {
            text: "agent answer".into(),
            path: None,
            kind: AgentMessageKind::Agent,
        });
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| editor.render(frame)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("YOU"));
        assert!(rendered.contains("CODEX"));

        let area = editor.agent_area.unwrap();
        editor
            .handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: area.x + 1,
                row: area.y + 1,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        assert_eq!(editor.agent_scroll, 3);
        editor
            .agent_panel_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(editor.agent_scroll, 0);
    }

    #[test]
    fn tracked_agent_edit_can_be_reverted() {
        let mut editor = Editor::new(Vec::new());
        let id = editor.current().id();
        editor
            .execute_agent_method(
                "edit.apply",
                &json!({"buffer_id":id,"revision":0,"start":0,"end":0,"text":"agent"}),
            )
            .unwrap();
        editor.revert_agent_changes();
        assert_eq!(editor.current().text(), "");
        assert!(editor.agent_modified.is_empty());
    }

    #[test]
    fn agent_revert_never_undoes_a_later_human_edit() {
        let mut editor = Editor::new(Vec::new());
        let id = editor.current().id();
        editor
            .execute_agent_method(
                "edit.apply",
                &json!({"buffer_id":id,"revision":0,"start":0,"end":0,"text":"agent"}),
            )
            .unwrap();
        editor.current_mut().insert_typed(" human");
        editor.revert_agent_changes();
        assert_eq!(editor.current().text(), "agent human");
        assert!(editor.agent_modified.contains_key(&id));
        assert!(editor.message.contains("skipped"));
    }

    #[test]
    fn built_in_agent_disk_changes_can_be_reverted_safely() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("agent.txt");
        fs::write(&path, "before").unwrap();
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor.agent_disk_before = editor.capture_workspace_files();
        fs::write(&path, "after").unwrap();
        editor.finish_agent_disk_changes();
        assert_eq!(editor.agent_disk_changes.len(), 1);
        editor.revert_agent_changes();
        assert_eq!(fs::read_to_string(path).unwrap(), "before");
    }

    #[test]
    fn built_in_agent_revert_preserves_later_human_disk_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("agent.txt");
        fs::write(&path, "before").unwrap();
        let mut editor = Editor::new(Vec::new());
        editor.explorer = Explorer::new(root.path().to_path_buf());
        editor.agent_disk_before = editor.capture_workspace_files();
        fs::write(&path, "agent").unwrap();
        editor.finish_agent_disk_changes();
        fs::write(&path, "human").unwrap();
        editor.revert_agent_changes();
        assert_eq!(fs::read_to_string(path).unwrap(), "human");
        assert_eq!(editor.agent_disk_changes.len(), 1);
    }
}
