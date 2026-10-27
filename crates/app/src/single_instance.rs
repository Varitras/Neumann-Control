//! One running app per folder. The settings, the log and the reports live next to the exe, so a
//! second app on the same folder would overwrite the first one's settings when it closes and
//! empty its log when it starts. Copies in other folders have their own files and may run side
//! by side. The lock is an OS file lock: it ends with the process, also after a crash.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Next to the exe; it stays there empty after the app closes.
pub const LOCK_FILE: &str = "neumann-control.lock";

/// Held for as long as the app runs.
pub struct InstanceLock {
    _file: File,
}

pub enum Claim {
    Ours(InstanceLock),
    /// Another app holds the folder.
    Taken,
    /// No lock file could be made (a read-only folder): the app runs, and saving fails there
    /// anyway, so it cannot overwrite another app's settings.
    NoLockFile(io::Error),
    /// The file system takes the file but not the lock (none known on Windows' own drives):
    /// the app runs and saves, without keeping a second app on the folder out.
    Unlockable(io::Error),
}

pub fn claim(directory: &Path) -> Claim {
    let opened = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK_FILE));
    let file = match opened {
        Ok(file) => file,
        Err(error) => return Claim::NoLockFile(error),
    };
    match file.try_lock() {
        Ok(()) => Claim::Ours(InstanceLock { _file: file }),
        Err(TryLockError::WouldBlock) => Claim::Taken,
        Err(TryLockError::Error(error)) => Claim::Unlockable(error),
    }
}

/// Brings the running app's main window to the front; false when no such window was found or
/// it could not be brought forward. Every copy has the same title, so with copies in other
/// folders running, the one found may be another folder's.
#[cfg(windows)]
pub fn show_running(title: &str) -> bool {
    use std::ptr::null;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the title is NUL-terminated and outlives the call; the handle is only passed on.
    unsafe {
        let window = FindWindowW(null(), title.as_ptr());
        if window.is_null() {
            return false;
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window) != 0
    }
}

/// Wayland lets no program bring another's window forward; the second start just ends.
#[cfg(target_os = "linux")]
pub fn show_running(_title: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_ours(claim: &Claim) -> bool {
        matches!(claim, Claim::Ours(_))
    }

    #[test]
    fn a_second_claim_on_the_same_folder_is_refused_until_the_first_ends() {
        let directory = tempfile::tempdir().unwrap();
        let first = claim(directory.path());
        assert!(is_ours(&first));
        assert!(matches!(claim(directory.path()), Claim::Taken));
        drop(first);
        assert!(is_ours(&claim(directory.path())));
    }

    #[test]
    fn another_folder_has_its_own_lock() {
        let one = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let first = claim(one.path());
        assert!(is_ours(&first));
        assert!(is_ours(&claim(other.path())));
    }

    #[test]
    fn a_folder_that_cannot_hold_the_lock_file_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");
        assert!(matches!(claim(&missing), Claim::NoLockFile(_)));
    }
}
