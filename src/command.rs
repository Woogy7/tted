#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewFile,
    Save,
    SaveAs,
    CloseFile,
    QuickOpen,
    ToggleExplorer,
    FocusMode,
    FindReplace,
    NextTab,
    PreviousTab,
    ToggleMarkdownReader,
    GitStatus,
    GitCurrentFileDiff,
    GitWorkspaceDiff,
    GitStageCurrentFile,
    GitUnstageCurrentFile,
    GitDiscardCurrentFile,
    GitCommit,
    ToggleProblems,
    LspHover,
    LspDefinition,
    LspCompletion,
    LspBack,
    LspReferences,
    LspRename,
    LspCodeActions,
    LspFormat,
    LspDocumentSymbols,
    LspWorkspaceSymbols,
    LspSignature,
    LspRestart,
    SplitRight,
    SplitDown,
    CloseSplit,
    FocusNextSplit,
    ToggleAgentPanel,
    AgentViewDiff,
    AgentAccept,
    AgentRevert,
    AgentAsk,
    AgentExplainSelection,
    AgentRefactorSelection,
    AgentWriteTests,
    AgentReviewDiff,
    ReloadConfig,
    ShowHelp,
    Quit,
}

impl Command {
    pub const ALL: [Self; 47] = [
        Self::NewFile,
        Self::Save,
        Self::SaveAs,
        Self::CloseFile,
        Self::QuickOpen,
        Self::ToggleExplorer,
        Self::FocusMode,
        Self::FindReplace,
        Self::NextTab,
        Self::PreviousTab,
        Self::ToggleMarkdownReader,
        Self::GitStatus,
        Self::GitCurrentFileDiff,
        Self::GitWorkspaceDiff,
        Self::GitStageCurrentFile,
        Self::GitUnstageCurrentFile,
        Self::GitDiscardCurrentFile,
        Self::GitCommit,
        Self::ToggleProblems,
        Self::LspHover,
        Self::LspDefinition,
        Self::LspCompletion,
        Self::LspBack,
        Self::LspReferences,
        Self::LspRename,
        Self::LspCodeActions,
        Self::LspFormat,
        Self::LspDocumentSymbols,
        Self::LspWorkspaceSymbols,
        Self::LspSignature,
        Self::LspRestart,
        Self::SplitRight,
        Self::SplitDown,
        Self::CloseSplit,
        Self::FocusNextSplit,
        Self::ToggleAgentPanel,
        Self::AgentViewDiff,
        Self::AgentAccept,
        Self::AgentRevert,
        Self::AgentAsk,
        Self::AgentExplainSelection,
        Self::AgentRefactorSelection,
        Self::AgentWriteTests,
        Self::AgentReviewDiff,
        Self::ReloadConfig,
        Self::ShowHelp,
        Self::Quit,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::NewFile => "file.new",
            Self::Save => "file.save",
            Self::SaveAs => "file.save_as",
            Self::CloseFile => "file.close",
            Self::QuickOpen => "workspace.quick_open",
            Self::ToggleExplorer => "workspace.toggle_explorer",
            Self::FocusMode => "view.focus_mode",
            Self::FindReplace => "editor.find_replace",
            Self::NextTab => "tab.next",
            Self::PreviousTab => "tab.previous",
            Self::ToggleMarkdownReader => "markdown.toggle_reader",
            Self::GitStatus => "git.status",
            Self::GitCurrentFileDiff => "git.diff_current_file",
            Self::GitWorkspaceDiff => "git.diff_workspace",
            Self::GitStageCurrentFile => "git.stage_current_file",
            Self::GitUnstageCurrentFile => "git.unstage_current_file",
            Self::GitDiscardCurrentFile => "git.discard_current_file",
            Self::GitCommit => "git.commit",
            Self::ToggleProblems => "view.toggle_problems",
            Self::LspHover => "lsp.hover",
            Self::LspDefinition => "lsp.definition",
            Self::LspCompletion => "lsp.completion",
            Self::LspBack => "navigation.back",
            Self::LspReferences => "lsp.references",
            Self::LspRename => "lsp.rename",
            Self::LspCodeActions => "lsp.code_actions",
            Self::LspFormat => "lsp.format",
            Self::LspDocumentSymbols => "lsp.document_symbols",
            Self::LspWorkspaceSymbols => "lsp.workspace_symbols",
            Self::LspSignature => "lsp.signature_help",
            Self::LspRestart => "lsp.restart",
            Self::SplitRight => "view.split_right",
            Self::SplitDown => "view.split_down",
            Self::CloseSplit => "view.close_split",
            Self::FocusNextSplit => "view.focus_next_split",
            Self::ToggleAgentPanel => "view.toggle_agent",
            Self::AgentViewDiff => "agent.view_diff",
            Self::AgentAccept => "agent.accept_changes",
            Self::AgentRevert => "agent.revert_changes",
            Self::AgentAsk => "agent.ask",
            Self::AgentExplainSelection => "agent.explain_selection",
            Self::AgentRefactorSelection => "agent.refactor_selection",
            Self::AgentWriteTests => "agent.write_tests",
            Self::AgentReviewDiff => "agent.review_diff",
            Self::ReloadConfig => "config.reload",
            Self::ShowHelp => "help.keybindings",
            Self::Quit => "app.quit",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::NewFile => "File: New File",
            Self::Save => "File: Save",
            Self::SaveAs => "File: Save As",
            Self::CloseFile => "File: Close Tab",
            Self::QuickOpen => "Workspace: Quick Open",
            Self::ToggleExplorer => "View: Toggle Explorer",
            Self::FocusMode => "View: Focus Mode",
            Self::FindReplace => "Editor: Find and Replace",
            Self::NextTab => "Tab: Next",
            Self::PreviousTab => "Tab: Previous",
            Self::ToggleMarkdownReader => "Markdown: Toggle Reader",
            Self::GitStatus => "Git: Open Status",
            Self::GitCurrentFileDiff => "Git: Open Current File Diff",
            Self::GitWorkspaceDiff => "Git: Open Workspace Diff",
            Self::GitStageCurrentFile => "Git: Stage Current File",
            Self::GitUnstageCurrentFile => "Git: Unstage Current File",
            Self::GitDiscardCurrentFile => "Git: Discard Current File Changes",
            Self::GitCommit => "Git: Commit Staged Changes",
            Self::ToggleProblems => "View: Toggle Problems",
            Self::LspHover => "Language: Hover",
            Self::LspDefinition => "Language: Go to Definition",
            Self::LspCompletion => "Language: Complete",
            Self::LspBack => "Navigation: Go Back",
            Self::LspReferences => "Language: Find References",
            Self::LspRename => "Language: Rename Symbol",
            Self::LspCodeActions => "Language: Code Actions",
            Self::LspFormat => "Language: Format Document",
            Self::LspDocumentSymbols => "Language: Document Symbols",
            Self::LspWorkspaceSymbols => "Language: Workspace Symbols",
            Self::LspSignature => "Language: Signature Help",
            Self::LspRestart => "Language: Restart Server",
            Self::SplitRight => "View: Split Right",
            Self::SplitDown => "View: Split Down",
            Self::CloseSplit => "View: Close Split",
            Self::FocusNextSplit => "View: Focus Next Split",
            Self::ToggleAgentPanel => "View: Toggle Agent Panel",
            Self::AgentViewDiff => "Agent: View Diff",
            Self::AgentAccept => "Agent: Accept Changes",
            Self::AgentRevert => "Agent: Revert Changes",
            Self::AgentAsk => "Agent: Ask",
            Self::AgentExplainSelection => "Agent: Explain Selection",
            Self::AgentRefactorSelection => "Agent: Refactor Selection",
            Self::AgentWriteTests => "Agent: Write Tests",
            Self::AgentReviewDiff => "Agent: Review Diff",
            Self::ReloadConfig => "Preferences: Reload Configuration",
            Self::ShowHelp => "Help: Keybindings",
            Self::Quit => "Application: Quit",
        }
    }
}

pub struct CommandPalette {
    query: String,
    matches: Vec<Command>,
    selected: usize,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Command::ALL.to_vec(),
            selected: 0,
        }
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn matches(&self) -> &[Command] {
        &self.matches
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn selected_command(&self) -> Option<Command> {
        self.matches.get(self.selected).copied()
    }
    pub fn push(&mut self, character: char) {
        self.query.push(character);
        self.refresh();
    }
    pub fn push_str(&mut self, text: &str) {
        self.query.push_str(text);
        self.refresh();
    }
    pub fn pop(&mut self) {
        self.query.pop();
        self.refresh();
    }
    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.matches.len().saturating_sub(1));
    }
    fn refresh(&mut self) {
        self.matches = Command::ALL
            .into_iter()
            .filter(|command| {
                fuzzy_contains(command.title(), &self.query)
                    || fuzzy_contains(command.id(), &self.query)
            })
            .collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
    }
}

fn fuzzy_contains(candidate: &str, query: &str) -> bool {
    let mut candidate = candidate.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|wanted| candidate.any(|found| found == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_stable_unique_ids() {
        let mut ids = Command::ALL.map(Command::id).to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Command::ALL.len());
    }

    #[test]
    fn palette_fuzzy_filters_titles_and_ids() {
        let mut palette = CommandPalette::new();
        palette.push_str("tgexpl");
        assert_eq!(palette.selected_command(), Some(Command::ToggleExplorer));
        let mut palette = CommandPalette::new();
        palette.push_str("file.save_as");
        assert_eq!(palette.selected_command(), Some(Command::SaveAs));
    }
}
