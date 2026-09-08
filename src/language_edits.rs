//! Validate complete language-server edit batches before changing any buffer.
use crate::{
    buffer::Buffer,
    lsp::{EditVersions, TextEdit},
};
use std::path::{Path, PathBuf};

struct PreparedEdit {
    existing: Option<usize>,
    opened: Option<Buffer>,
    edits: Vec<TextEdit>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

pub(crate) fn apply(
    buffers: &mut Vec<Buffer>,
    changes: Vec<(PathBuf, Vec<TextEdit>)>,
    versions: &EditVersions,
) -> Result<usize, String> {
    let mut prepared = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for (path, edits) in changes {
        if edits.is_empty() {
            continue;
        }
        if paths.iter().any(|other| same_path(other, &path)) {
            return Err("duplicate file in language-server edits".into());
        }
        paths.push(path.clone());
        let existing = buffers
            .iter()
            .position(|buffer| buffer.path().is_some_and(|open| same_path(open, &path)));
        let expected = versions
            .iter()
            .find(|(open, _)| same_path(open, &path))
            .map(|(_, version)| *version);
        let opened = if existing.is_none() {
            if expected.is_some() {
                return Err(
                    "edited file was closed while waiting; request the action again".into(),
                );
            }
            Some(Buffer::open(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        } else {
            None
        };
        let buffer = existing
            .map(|index| &buffers[index])
            .or(opened.as_ref())
            .expect("existing or newly opened buffer");
        if existing.is_some() && expected != Some((buffer.id(), buffer.revision())) {
            return Err(
                "file changed while waiting for the language server; request the action again"
                    .into(),
            );
        }
        if buffer.check_external_change().map_err(|e| e.to_string())?
            != crate::buffer::ExternalChange::None
        {
            return Err(
                "file changed on disk; resolve the external change before applying language edits"
                    .into(),
            );
        }
        buffer.validate_text_edits(&edits).map_err(str::to_owned)?;
        prepared.push(PreparedEdit {
            existing,
            opened,
            edits,
        });
    }
    let count = prepared.len();
    for edit in prepared {
        let index = match edit.existing {
            Some(index) => index,
            None => {
                buffers.push(edit.opened.expect("validated new buffer"));
                buffers.len() - 1
            }
        };
        let buffer = &mut buffers[index];
        buffer
            .apply_text_edits(&edit.edits)
            .expect("validated edits on the UI thread");
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_stale_and_unreadable_targets_without_partial_edits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a.rs");
        std::fs::write(&path, "original").unwrap();
        let mut buffers = vec![Buffer::open(&path).unwrap()];
        let versions =
            EditVersions::from([(path.clone(), (buffers[0].id(), buffers[0].revision()))]);
        let change = vec![(path.clone(), vec![(0, 0, 0, 8, "formatted".into())])];
        buffers[0].insert("human");
        assert!(apply(&mut buffers, change.clone(), &versions).is_err());
        assert_eq!(buffers[0].text(), "humanoriginal");
        let versions =
            EditVersions::from([(path.clone(), (buffers[0].id(), buffers[0].revision()))]);
        let mut changes = change;
        changes.push((
            directory.path().join("missing.rs"),
            vec![(0, 0, 0, 0, "new".into())],
        ));
        assert!(apply(&mut buffers, changes, &versions).is_err());
        assert_eq!(buffers[0].text(), "humanoriginal");
    }
    #[test]
    fn valid_batch_updates_multiple_files_as_undoable_edits() {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a.rs");
        let b = directory.path().join("b.rs");
        std::fs::write(&a, "old").unwrap();
        std::fs::write(&b, "old").unwrap();
        let mut buffers = vec![Buffer::open(&a).unwrap()];
        let versions = EditVersions::from([(a.clone(), (buffers[0].id(), 0))]);
        assert_eq!(
            apply(
                &mut buffers,
                vec![
                    (a, vec![(0, 0, 0, 3, "new".into())]),
                    (b, vec![(0, 0, 0, 3, "new".into())])
                ],
                &versions
            )
            .unwrap(),
            2
        );
        for buffer in &mut buffers {
            assert_eq!(buffer.text(), "new");
            buffer.undo();
            assert_eq!(buffer.text(), "old");
        }
    }
}
