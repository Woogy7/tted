use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerRow {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

#[derive(Debug)]
pub enum ExplorerAction {
    None,
    Open(PathBuf),
}

pub struct Explorer {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    rows: Vec<ExplorerRow>,
    selected: usize,
    scroll: usize,
    focused: bool,
}

impl Explorer {
    pub fn new(root: PathBuf) -> Self {
        let mut explorer = Self {
            root,
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            focused: false,
        };
        explorer.refresh();
        explorer
    }

    pub fn rows(&self) -> &[ExplorerRow] {
        &self.rows
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> usize {
        self.scroll
    }
    pub fn focused(&self) -> bool {
        self.focused
    }
    pub fn selected_path(&self) -> Option<&Path> {
        self.rows.get(self.selected).map(|row| row.path.as_path())
    }
    pub fn operation_directory(&self) -> &Path {
        self.rows
            .get(self.selected)
            .and_then(|row| {
                if row.is_dir {
                    Some(row.path.as_path())
                } else {
                    row.path.parent()
                }
            })
            .unwrap_or(&self.root)
    }
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn refresh(&mut self) {
        let selected_path = self.rows.get(self.selected).map(|row| row.path.clone());
        let mut rows = Vec::new();
        self.collect_directory(&self.root, 0, &mut rows);
        self.rows = rows;
        self.selected = selected_path
            .and_then(|path| self.rows.iter().position(|row| row.path == path))
            .unwrap_or_else(|| self.selected.min(self.rows.len().saturating_sub(1)));
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
    }

    pub fn move_selection(&mut self, delta: isize, height: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
        self.ensure_visible(height);
    }

    pub fn select_visible(&mut self, visible_index: usize, height: usize) {
        self.selected = (self.scroll + visible_index).min(self.rows.len().saturating_sub(1));
        self.ensure_visible(height);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn select_last(&mut self, height: usize) {
        self.selected = self.rows.len().saturating_sub(1);
        self.ensure_visible(height);
    }

    pub fn scroll_by(&mut self, delta: isize, height: usize) {
        let max_scroll = self.rows.len().saturating_sub(height.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);
    }

    pub fn activate_selected(&mut self) -> ExplorerAction {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return ExplorerAction::None;
        };
        if row.is_dir {
            self.toggle_path(&row.path);
            ExplorerAction::None
        } else {
            ExplorerAction::Open(row.path)
        }
    }

    pub fn expand_selected(&mut self) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };
        if row.is_dir && !row.expanded {
            self.expanded.insert(row.path);
            self.refresh();
        }
    }

    pub fn collapse_or_parent(&mut self, height: usize) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };
        if row.is_dir && row.expanded {
            self.expanded.remove(&row.path);
            self.refresh();
        } else if let Some(parent) = row.path.parent() {
            if let Some(index) = self
                .rows
                .iter()
                .position(|candidate| candidate.path == parent)
            {
                self.selected = index;
                self.ensure_visible(height);
            }
        }
    }

    pub fn ensure_visible(&mut self, height: usize) {
        let height = height.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
    }

    fn toggle_path(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
        self.refresh();
    }

    fn collect_directory(&self, directory: &Path, depth: usize, rows: &mut Vec<ExplorerRow>) {
        if rows.len() >= 5_000 {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| (!entry.path().is_dir(), entry.file_name()));
        for entry in entries {
            if rows.len() >= 5_000 {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if excluded(&name) {
                continue;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            let expanded = is_dir && self.expanded.contains(&path);
            rows.push(ExplorerRow {
                path: path.clone(),
                depth,
                is_dir,
                expanded,
            });
            if expanded {
                self.collect_directory(&path, depth + 1, rows);
            }
        }
    }
}

fn excluded(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist" | "build")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_is_lazy_and_directories_expand() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.path().join("README.md"), "read me").unwrap();
        let mut explorer = Explorer::new(root.path().to_path_buf());
        assert_eq!(explorer.rows().len(), 2);
        assert!(explorer.rows()[0].is_dir);
        explorer.activate_selected();
        assert_eq!(explorer.rows().len(), 3);
        assert_eq!(explorer.rows()[1].depth, 1);
        explorer.activate_selected();
        assert_eq!(explorer.rows().len(), 2);
    }

    #[test]
    fn keyboard_selection_scrolls_and_opens_file() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c", "d"] {
            fs::write(root.path().join(name), name).unwrap();
        }
        let mut explorer = Explorer::new(root.path().to_path_buf());
        explorer.move_selection(3, 2);
        assert_eq!(explorer.selected(), 3);
        assert_eq!(explorer.scroll(), 2);
        assert!(matches!(
            explorer.activate_selected(),
            ExplorerAction::Open(_)
        ));
    }

    #[test]
    fn ignores_hidden_and_build_directories() {
        let root = tempfile::tempdir().unwrap();
        for directory in [".git", "target", "node_modules"] {
            fs::create_dir(root.path().join(directory)).unwrap();
        }
        fs::write(root.path().join("visible.txt"), "ok").unwrap();
        let explorer = Explorer::new(root.path().to_path_buf());
        assert_eq!(explorer.rows().len(), 1);
        assert_eq!(explorer.rows()[0].path.file_name().unwrap(), "visible.txt");
    }
}
