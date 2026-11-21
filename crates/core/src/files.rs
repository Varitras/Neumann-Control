//! Replacing a file whole: through a temporary file beside it, so a write that breaks off (a
//! crash, a full disk) leaves the old file, never a truncated one.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const TEMPORARY_SUFFIX: &str = ".tmp";

/// Writes `contents` to `path`, replacing what was there only once all of it is on disk.
pub fn replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = with_suffix(path, TEMPORARY_SUFFIX);
    let mut file = File::create(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// `path` with `suffix` added to its file name, e.g. `settings.json.tmp`.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
