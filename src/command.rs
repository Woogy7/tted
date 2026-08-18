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
    ShowHelp,
    Quit,
}

impl Command {
    pub const ALL: [Self; 13] = [
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
            Self::ShowHelp => "help.keybindings",
            Self::Quit => "app.quit",
        }
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
