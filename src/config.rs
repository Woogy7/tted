use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{diagnostics, lsp::LanguageServerConfig};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct EditorConfig {
    pub tab_width: usize,
    pub use_spaces: bool,
    pub line_numbers: bool,
    pub word_wrap: bool,
    pub theme: String,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_width: 4,
            use_spaces: true,
            line_numbers: true,
            word_wrap: false,
            theme: "base16-eighties.dark".into(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub language_servers: HashMap<String, LanguageServerConfig>,
    pub keybindings: HashMap<String, String>,
    pub agent: AgentConfig,
    pub explorer: ExplorerConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ExplorerConfig {
    pub show_hidden: bool,
    pub show_build_directories: bool,
    pub max_entries: usize,
}
impl Default for ExplorerConfig {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_build_directories: false,
            max_entries: 5_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentConfig {
    pub enabled: bool,
    pub allow_read: bool,
    pub allow_write: bool,
    pub allow_file_create: bool,
    pub allow_file_delete: bool,
    pub allow_commands: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_read: true,
            allow_write: false,
            allow_file_create: false,
            allow_file_delete: false,
            allow_commands: false,
        }
    }
}

impl Config {
    pub fn load(workspace: &Path) -> Self {
        let path = std::env::var_os("TTED_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join(".tted.toml"));
        let Ok(source) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&source) {
            Ok(config) => config,
            Err(error) => {
                diagnostics::log(&format!(
                    "configuration error in {}: {error}",
                    path.display()
                ));
                Self::default()
            }
        }
    }

    pub fn language_server(&self, path: &Path) -> Option<&LanguageServerConfig> {
        let extension = path.extension()?.to_str()?;
        self.language_servers.get(extension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_editor_and_language_server_settings() {
        let config: Config = toml::from_str(
            r#"
            [editor]
            tab_width = 2
            word_wrap = true

            [language_servers.rs]
            command = "rust-analyzer"
            language_id = "rust"
        "#,
        )
        .unwrap();
        assert_eq!(config.editor.tab_width, 2);
        assert!(config.editor.word_wrap);
        assert_eq!(config.language_servers["rs"].command, "rust-analyzer");
    }

    #[test]
    fn parses_explorer_agent_and_keybinding_settings() {
        let config: Config = toml::from_str(
            r#"
            [explorer]
            show_hidden = true
            max_entries = 42
            [agent]
            allow_write = true
            [keybindings]
            "alt+p" = "workspace.quick_open"
        "#,
        )
        .unwrap();
        assert!(config.explorer.show_hidden);
        assert_eq!(config.explorer.max_entries, 42);
        assert!(config.agent.allow_write);
        assert_eq!(config.keybindings["alt+p"], "workspace.quick_open");
    }
}
