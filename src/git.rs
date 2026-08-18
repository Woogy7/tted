use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub root: Option<PathBuf>,
    pub branch: Option<String>,
    pub files: HashMap<PathBuf, char>,
}

impl GitSnapshot {
    pub fn is_repository(&self) -> bool {
        self.root.is_some()
    }
    pub fn is_dirty(&self) -> bool {
        !self.files.is_empty()
    }
    pub fn decoration(&self, path: &Path) -> Option<char> {
        let root = self.root.as_ref()?;
        let relative = path.strip_prefix(root).ok()?;
        self.files.get(relative).copied()
    }
}

pub struct GitService {
    workspace: PathBuf,
    receiver: Receiver<GitSnapshot>,
    sender: mpsc::Sender<GitSnapshot>,
    pending: bool,
    last_request: Instant,
    snapshot: GitSnapshot,
}

impl GitService {
    pub fn new(workspace: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut service = Self {
            workspace,
            receiver,
            sender,
            pending: false,
            last_request: Instant::now() - Duration::from_secs(10),
            snapshot: GitSnapshot::default(),
        };
        service.request_refresh();
        service
    }

    pub fn snapshot(&self) -> &GitSnapshot {
        &self.snapshot
    }

    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(snapshot) = self.receiver.try_recv() {
            self.snapshot = snapshot;
            self.pending = false;
            changed = true;
        }
        if !self.pending && self.last_request.elapsed() >= Duration::from_secs(2) {
            self.request_refresh();
        }
        changed
    }

    pub fn request_refresh(&mut self) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.last_request = Instant::now();
        let workspace = self.workspace.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(read_snapshot(&workspace));
        });
    }
}

fn read_snapshot(workspace: &Path) -> GitSnapshot {
    let root = Command::new("git")
        .args([
            "-C",
            &workspace.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|root| PathBuf::from(root.trim()));
    let Some(root) = root else {
        return GitSnapshot::default();
    };
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "--branch",
            "-z",
        ])
        .output();
    let Ok(output) = output else {
        return GitSnapshot {
            root: Some(root),
            ..GitSnapshot::default()
        };
    };
    let (branch, files) = parse_status(&output.stdout);
    GitSnapshot {
        root: Some(root),
        branch,
        files,
    }
}

fn parse_status(output: &[u8]) -> (Option<String>, HashMap<PathBuf, char>) {
    let mut branch = None;
    let mut files = HashMap::new();
    let records = output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        let text = String::from_utf8_lossy(record);
        if let Some(header) = text.strip_prefix("## ") {
            branch = Some(
                header
                    .strip_prefix("No commits yet on ")
                    .or_else(|| header.strip_prefix("Initial commit on "))
                    .unwrap_or(header)
                    .split("...")
                    .next()
                    .unwrap_or(header)
                    .to_owned(),
            );
            index += 1;
            continue;
        }
        if text.len() < 4 {
            index += 1;
            continue;
        }
        let status = &text[..2];
        let path = &text[3..];
        let decoration = if status == "??" {
            '?'
        } else if status.contains('A') {
            'A'
        } else if status.contains('D') {
            'D'
        } else {
            'M'
        };
        files.insert(PathBuf::from(path), decoration);
        index += 1 + usize::from(status.contains('R') || status.contains('C'));
    }
    (branch, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_and_common_file_states() {
        let data =
            b"## main...origin/main\0 M src/editor.rs\0A  added.txt\0?? notes.md\0D  old.rs\0";
        let (branch, files) = parse_status(data);
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(files[Path::new("src/editor.rs")], 'M');
        assert_eq!(files[Path::new("added.txt")], 'A');
        assert_eq!(files[Path::new("notes.md")], '?');
        assert_eq!(files[Path::new("old.rs")], 'D');
    }

    #[test]
    fn parses_initial_branch_and_skips_rename_source_record() {
        let data = b"## No commits yet on trunk\0R  new.rs\0old.rs\0";
        let (branch, files) = parse_status(data);
        assert_eq!(branch.as_deref(), Some("trunk"));
        assert_eq!(files.get(Path::new("new.rs")), Some(&'M'));
        assert_eq!(files.len(), 1);
    }
}
