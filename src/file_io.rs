//! Persistence primitives shared by buffers and save dialogs.
use std::os::unix::fs::MetadataExt;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    device: u64,
    inode: u64,
    changed: (i64, i64),
}

pub(crate) fn file_stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    match fs::metadata(path) {
        Ok(m) => Ok(Some(FileStamp {
            modified: m.modified().ok(),
            len: m.len(),
            device: m.dev(),
            inode: m.ino(),
            changed: (m.ctime(), m.ctime_nsec()),
        })),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub(crate) fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// A unique sibling temporary file keeps incomplete writes away from the target.
/// Existing symlinks continue to point at their original targets.
pub(crate) fn atomic_write(
    path: &Path,
    text: &[u8],
    expected: Option<FileStamp>,
) -> io::Result<()> {
    let destination = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Do not silently replace a dangling link.
            if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "save target is a dangling symlink",
                ));
            }
            absolute_path(path)?
        }
        Err(e) => return Err(e),
    };
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("save target has no parent"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".tted-save-")
        .tempfile_in(parent)?;
    temporary.write_all(text)?;
    if let Ok(metadata) = fs::metadata(&destination) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.as_file().sync_all()?;
    if file_stamp(path)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "file changed on disk; reload or keep the editor version before saving",
        ));
    }
    if expected.is_some() {
        temporary.persist(&destination).map_err(|e| e.error)?;
    } else {
        temporary
            .persist_noclobber(&destination)
            .map_err(|e| e.error)?;
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Read a coherent UTF-8 document, retrying if an external writer changes it.
pub(crate) fn read_document(path: &Path) -> io::Result<(String, bool, Option<FileStamp>)> {
    for _ in 0..3 {
        let before = file_stamp(path)?;
        let bytes = fs::read(path)?;
        let after = file_stamp(path)?;
        if before != after {
            continue;
        }
        let source = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "TTED only edits UTF-8 text")
        })?;
        let crlf = source.contains("\r\n");
        return Ok((
            if crlf {
                source.replace("\r\n", "\n")
            } else {
                source
            },
            crlf,
            after,
        ));
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "file is changing; try reloading again",
    ))
}
