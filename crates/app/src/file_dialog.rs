//! Choosing a file for the EQ import and export: Windows' own dialog, a stand-in in the tests.

use std::path::PathBuf;

pub trait FileDialog {
    /// The text file to read; `None` when the user cancels.
    fn open(&mut self) -> Option<PathBuf>;
    /// Where to write `suggested` (a file name); `None` when the user cancels.
    fn save(&mut self, suggested: &str) -> Option<PathBuf>;
}

#[cfg(windows)]
pub use native::NativeFileDialog;

#[cfg(windows)]
mod native {
    use std::path::PathBuf;

    use super::FileDialog;

    const TEXT_FILTER_NAME: &str = "Text";
    const TEXT_EXTENSIONS: [&str; 1] = ["txt"];

    #[derive(Default)]
    pub struct NativeFileDialog;

    impl FileDialog for NativeFileDialog {
        fn open(&mut self) -> Option<PathBuf> {
            rfd::FileDialog::new()
                .add_filter(TEXT_FILTER_NAME, &TEXT_EXTENSIONS)
                .pick_file()
        }

        fn save(&mut self, suggested: &str) -> Option<PathBuf> {
            rfd::FileDialog::new()
                .add_filter(TEXT_FILTER_NAME, &TEXT_EXTENSIONS)
                .set_file_name(suggested)
                .save_file()
        }
    }
}

/// Elsewhere there is no dialog: import and export find no file.
#[cfg(not(windows))]
#[derive(Default)]
pub struct NativeFileDialog;

#[cfg(not(windows))]
impl FileDialog for NativeFileDialog {
    fn open(&mut self) -> Option<PathBuf> {
        None
    }

    fn save(&mut self, _suggested: &str) -> Option<PathBuf> {
        None
    }
}
