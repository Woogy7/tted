use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct QuickOpen {
    root: PathBuf,
    files: Vec<PathBuf>,
    matches: Vec<usize>,
    query: String,
    selected: usize,
}

impl QuickOpen {
    pub fn new(root: PathBuf) -> Self {
        let mut files = Vec::new();
        collect_files(&root, &mut files);
        files.sort();
        let matches = (0..files.len()).collect();
        Self {
            root,
            files,
            matches,
            query: String::new(),
            selected: 0,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn matches(&self) -> impl Iterator<Item = &Path> {
        self.matches
            .iter()
            .map(|index| self.files[*index].as_path())
    }
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn push(&mut self, character: char) {
        self.query.push(character);
        self.update_matches();
    }

    pub fn push_str(&mut self, text: &str) {
        self.query.push_str(text);
        self.update_matches();
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.update_matches();
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.matches.len().saturating_sub(1));
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.matches
            .get(self.selected)
            .map(|index| self.files[*index].clone())
    }

    pub fn display_path<'a>(&'a self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.root).unwrap_or(path)
    }

    fn update_matches(&mut self) {
        let mut ranked = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, path)| {
                let relative = path.strip_prefix(&self.root).unwrap_or(path);
                fuzzy_score(&relative.to_string_lossy(), &self.query).map(|score| (score, index))
            })
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(score, index)| (*score, self.files[*index].clone()));
        self.matches = ranked.into_iter().map(|(_, index)| index).collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
    }
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    if files.len() >= 10_000 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if files.len() >= 10_000 {
            break;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || matches!(name.as_ref(), "target" | "node_modules" | "dist" | "build")
        {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_files(&path, files);
        } else if file_type.is_file() {
            files.push(path);
        }
    }
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(candidate.len());
    }
    let candidate = candidate.to_lowercase();
    let query = query.to_lowercase();
    let mut position = 0usize;
    let mut score = 0usize;
    let mut previous = None;
    for needle in query.chars() {
        let offset = candidate[position..].find(needle)?;
        let found = position + offset;
        score += offset;
        if previous.is_some_and(|previous| found == previous + needle.len_utf8()) {
            score = score.saturating_sub(1);
        }
        previous = Some(found);
        position = found + needle.len_utf8();
    }
    Some(score + candidate.len().saturating_sub(query.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_requires_order_and_rewards_contiguous_text() {
        assert!(fuzzy_score("src/editor.rs", "sedr").is_some());
        assert!(fuzzy_score("src/editor.rs", "zxy").is_none());
        assert!(fuzzy_score("editor.rs", "editor") < fuzzy_score("edit_other.rs", "editor"));
    }

    #[test]
    fn scans_workspace_and_updates_selection() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/editor.rs"), "").unwrap();
        fs::write(root.path().join("README.md"), "").unwrap();
        let mut picker = QuickOpen::new(root.path().to_path_buf());
        picker.push_str("edrs");
        assert_eq!(
            picker.selected_path().unwrap(),
            root.path().join("src/editor.rs")
        );
    }
}
