use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use crate::diagnostics;

static COMMAND_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified_ms: u128,
}

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub root: Option<PathBuf>,
    pub branch: Option<String>,
    pub files: HashMap<PathBuf, char>,
    pub lines: HashMap<PathBuf, HashMap<usize, char>>,
    diff_text: String,
    stamps: HashMap<PathBuf, FileStamp>,
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
    pub fn line_decoration(&self, path: &Path, line: usize) -> Option<char> {
        let root = self.root.as_ref()?;
        let relative = path.strip_prefix(root).ok()?;
        self.lines.get(relative)?.get(&line).copied()
    }
    pub fn status_text(&self) -> String {
        let Some(root) = &self.root else {
            return "Not inside a Git repository.\n".into();
        };
        let mut text = format!(
            "Repository: {}\nBranch: {}\n\n",
            root.display(),
            self.branch.as_deref().unwrap_or("(detached HEAD)")
        );
        if self.files.is_empty() {
            text.push_str("Working tree clean.\n");
            return text;
        }
        let mut files = self.files.iter().collect::<Vec<_>>();
        files.sort_unstable_by_key(|(path, _)| *path);
        text.push_str("Changes:\n");
        for (path, status) in files {
            text.push_str(&format!("  {status}  {}\n", path.display()));
        }
        text
    }
    pub fn workspace_diff(&self) -> String {
        if self.diff_text.is_empty() {
            "No tracked workspace changes. Untracked files appear in Git Status.\n".into()
        } else {
            self.diff_text.clone()
        }
    }
    pub fn file_diff(&self, path: &Path) -> String {
        let Some(root) = &self.root else {
            return "Not inside a Git repository.\n".into();
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return "The current file is outside the Git repository.\n".into();
        };
        let relative = relative.to_string_lossy();
        let sections = self.diff_text.split("diff --git ").skip(1);
        let matching = sections
            .filter(|section| {
                section.contains(&format!("+++ b/{relative}\n"))
                    || section.contains(&format!("--- a/{relative}\n"))
            })
            .map(|section| format!("diff --git {section}"))
            .collect::<String>();
        if matching.is_empty() {
            format!("No tracked changes for {}.\n", relative)
        } else {
            matching
        }
    }
}

pub struct GitService {
    workspace: PathBuf,
    receiver: Receiver<GitSnapshot>,
    sender: mpsc::Sender<GitSnapshot>,
    pending: bool,
    last_request: Instant,
    snapshot: GitSnapshot,
    operation_sender: mpsc::Sender<String>,
    operation_receiver: Receiver<String>,
    operation_pending: bool,
}

impl GitService {
    pub fn new(workspace: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (operation_sender, operation_receiver) = mpsc::channel();
        let mut service = Self {
            workspace,
            receiver,
            sender,
            pending: false,
            last_request: Instant::now() - Duration::from_secs(10),
            snapshot: GitSnapshot::default(),
            operation_sender,
            operation_receiver,
            operation_pending: false,
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
        if !self.pending && self.last_request.elapsed() >= Duration::from_secs(5) {
            self.request_refresh();
        }
        changed
    }

    pub fn take_operation_message(&mut self) -> Option<String> {
        let message = self.operation_receiver.try_recv().ok()?;
        self.operation_pending = false;
        self.request_refresh();
        Some(message)
    }

    pub fn stage(&mut self, path: &Path) -> bool {
        self.start_file_operation(path, "stage", vec!["add", "--"])
    }

    pub fn unstage(&mut self, path: &Path) -> bool {
        self.start_file_operation(path, "unstage", vec!["restore", "--staged", "--"])
    }

    pub fn discard(&mut self, path: &Path) -> bool {
        self.start_file_operation(path, "discard", vec!["restore", "--worktree", "--"])
    }

    pub fn commit(&mut self, message: String) -> bool {
        let Some(root) = self.snapshot.root.clone() else {
            return false;
        };
        self.start_operation(root, "commit", vec!["commit".into(), "-m".into(), message])
    }

    fn start_file_operation(&mut self, path: &Path, label: &'static str, args: Vec<&str>) -> bool {
        let Some(root) = self.snapshot.root.clone() else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            return false;
        };
        let mut owned = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        owned.push(relative.to_string_lossy().into_owned());
        self.start_operation(root, label, owned)
    }

    fn start_operation(&mut self, root: PathBuf, label: &'static str, args: Vec<String>) -> bool {
        if self.operation_pending {
            return false;
        }
        self.operation_pending = true;
        let sender = self.operation_sender.clone();
        thread::spawn(move || {
            let arguments = args.iter().map(String::as_str).collect::<Vec<_>>();
            let success = run_git(&root, &arguments, Duration::from_secs(15)).is_some();
            let message = if success {
                format!("Git {label} completed")
            } else {
                format!("Git {label} failed; see diagnostics log")
            };
            diagnostics::log(&message);
            let _ = sender.send(message);
        });
        true
    }

    pub fn request_refresh(&mut self) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.last_request = Instant::now();
        let workspace = self.workspace.clone();
        let sender = self.sender.clone();
        let previous = self.snapshot.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let snapshot = read_snapshot(&workspace, &previous);
            diagnostics::log(&format!(
                "git refresh completed in {}ms: repo={} files={} line_maps={}",
                started.elapsed().as_millis(),
                snapshot.is_repository(),
                snapshot.files.len(),
                snapshot.lines.len()
            ));
            let _ = sender.send(snapshot);
        });
    }
}

fn read_snapshot(workspace: &Path, previous: &GitSnapshot) -> GitSnapshot {
    let root = run_git(
        workspace,
        &["rev-parse", "--show-toplevel"],
        Duration::from_secs(2),
    )
    .and_then(|output| String::from_utf8(output).ok())
    .map(|root| PathBuf::from(root.trim()));
    let Some(root) = root else {
        return GitSnapshot::default();
    };
    let Some(output) = run_git(
        &root,
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
        Duration::from_secs(3),
    ) else {
        return GitSnapshot {
            root: Some(root),
            ..GitSnapshot::default()
        };
    };
    let (branch, files) = parse_status(&output);
    let stamps = collect_stamps(&root, &files);
    let unchanged = previous.root.as_ref() == Some(&root)
        && previous.files == files
        && previous.stamps == stamps;
    let (mut lines, diff_text) = if unchanged {
        (previous.lines.clone(), previous.diff_text.clone())
    } else {
        let diff = run_git(
            &root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--unified=0",
                "HEAD",
                "--",
            ],
            Duration::from_secs(4),
        )
        .unwrap_or_default();
        (
            parse_diff(&diff),
            String::from_utf8_lossy(&diff).into_owned(),
        )
    };
    if !unchanged {
        for (path, status) in &files {
            if *status == '?' {
                let count = std::fs::read_to_string(root.join(path))
                    .map_or(1, |text| text.lines().count().max(1));
                lines.insert(path.clone(), (1..=count).map(|line| (line, 'A')).collect());
            }
        }
    }
    GitSnapshot {
        root: Some(root),
        branch,
        files,
        lines,
        diff_text,
        stamps,
    }
}

fn collect_stamps(root: &Path, files: &HashMap<PathBuf, char>) -> HashMap<PathBuf, FileStamp> {
    files
        .keys()
        .filter_map(|path| {
            let metadata = fs::metadata(root.join(path)).ok()?;
            let modified_ms = metadata
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis();
            Some((
                path.clone(),
                FileStamp {
                    len: metadata.len(),
                    modified_ms,
                },
            ))
        })
        .collect()
}

fn run_git(root: &Path, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let id = COMMAND_ID.fetch_add(1, Ordering::Relaxed);
    let output_path =
        std::env::temp_dir().join(format!("tted-git-{}-{id}.out", std::process::id()));
    let output_file = File::create(&output_path).ok()?;
    let started = Instant::now();
    diagnostics::log(&format!("git start: {}", args.join(" ")));
    let mut child = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_file))
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&output_path);
            diagnostics::log(&format!("git spawn error: {error}"));
            return None;
        }
    };
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                diagnostics::log(&format!(
                    "git timeout after {}ms: {}",
                    timeout.as_millis(),
                    args.join(" ")
                ));
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Err(error) => {
                diagnostics::log(&format!("git wait error: {error}"));
                break false;
            }
        }
    };
    let mut output = Vec::new();
    if success {
        let _ = File::open(&output_path).and_then(|mut file| file.read_to_end(&mut output));
    }
    let _ = fs::remove_file(&output_path);
    diagnostics::log(&format!(
        "git end: success={success} elapsed={}ms bytes={}",
        started.elapsed().as_millis(),
        output.len()
    ));
    success.then_some(output)
}

fn parse_diff(output: &[u8]) -> HashMap<PathBuf, HashMap<usize, char>> {
    let text = String::from_utf8_lossy(output);
    let mut result = HashMap::<PathBuf, HashMap<usize, char>>::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current = Some(PathBuf::from(path));
            continue;
        }
        if !line.starts_with("@@ ") {
            continue;
        }
        let Some(path) = &current else {
            continue;
        };
        let mut ranges = line
            .split_whitespace()
            .filter(|part| part.starts_with('-') || part.starts_with('+'));
        let old = ranges.next().and_then(parse_hunk_range);
        let new = ranges.next().and_then(parse_hunk_range);
        let (Some((_, old_count)), Some((new_start, new_count))) = (old, new) else {
            continue;
        };
        let markers = result.entry(path.clone()).or_default();
        if new_count == 0 {
            markers.insert(new_start.max(1), 'D');
        } else {
            let marker = if old_count == 0 { 'A' } else { 'M' };
            for number in new_start..new_start + new_count {
                markers.insert(number, marker);
            }
        }
    }
    result
}

fn parse_hunk_range(range: &str) -> Option<(usize, usize)> {
    let range = range.get(1..)?;
    let mut parts = range.split(',');
    let start = parts.next()?.parse().ok()?;
    let count = parts.next().map_or(Some(1), |count| count.parse().ok())?;
    Some((start, count))
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

    #[test]
    fn parses_added_modified_and_deleted_hunks() {
        let diff = b"diff --git a/src/a.rs b/src/a.rs\n+++ b/src/a.rs\n@@ -2 +2 @@\n-old\n+new\n@@ -5,0 +6,2 @@\n+a\n+b\n@@ -10,2 +11,0 @@\n-x\n-y\n";
        let lines = parse_diff(diff);
        let file = &lines[Path::new("src/a.rs")];
        assert_eq!(file[&2], 'M');
        assert_eq!(file[&6], 'A');
        assert_eq!(file[&7], 'A');
        assert_eq!(file[&11], 'D');
    }

    #[test]
    fn formats_status_and_extracts_one_file_diff() {
        let diff_text = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/src/b.rs b/src/b.rs\n--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-x\n+y\n";
        let snapshot = GitSnapshot {
            root: Some(PathBuf::from("/repo")),
            branch: Some("main".into()),
            files: HashMap::from([
                (PathBuf::from("src/b.rs"), 'M'),
                (PathBuf::from("src/a.rs"), 'M'),
            ]),
            diff_text: diff_text.into(),
            ..GitSnapshot::default()
        };
        let status = snapshot.status_text();
        assert!(status.contains("Branch: main"));
        assert!(status.find("src/a.rs").unwrap() < status.find("src/b.rs").unwrap());
        let file = snapshot.file_diff(Path::new("/repo/src/a.rs"));
        assert!(file.contains("+new"));
        assert!(!file.contains("src/b.rs"));
    }
}
