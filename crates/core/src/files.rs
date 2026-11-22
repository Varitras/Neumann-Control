//! Replacing a file whole: through a temporary file beside it, so a write that breaks off (a
//! crash, a full disk) leaves the old file, never a truncated one.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const TEMPORARY_SUFFIX: &str = ".tmp";

/// Writes `contents` to `path`, replacing what was there only once all of it is on disk.
/// A failure removes the temporary file again.
pub fn replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = with_suffix(path, TEMPORARY_SUFFIX);
    let written = write_through(&temporary, contents).and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn write_through(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

/// `path` with `suffix` added to its file name, e.g. `settings.json.tmp`.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A replace that fails leaves the old file and no temporary one beside it: the temporary
    /// file stayed next to the user's file.
    #[test]
    fn a_failed_replace_leaves_no_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        // A folder in the way: the temporary file is written, the rename fails.
        let target = directory.path().join("export.txt");
        fs::create_dir(&target).unwrap();
        assert!(replace(&target, b"new").is_err());
        let left: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }
}
