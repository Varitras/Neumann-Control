//! The EQ window's import and export of filter files (Equalizer APO, REW): reading and writing
//! the file here, the format in `eq_file`, the preview and the write as for an EQ preset.

use std::fs;
use std::path::Path;

use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::EqPreset;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::eq::PendingImport;
use super::NeumannApp;
use crate::eq::bands_of;
use crate::eq_file::{self, MAX_FILE_BYTES};
use crate::presets::eq_plan;
use crate::texts;

/// Characters Windows does not allow in a file name; a container label like "A/B" has some.
const FORBIDDEN_IN_FILE_NAMES: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const FILE_NAME_REPLACEMENT: char = '-';
const FILE_EXTENSION: &str = "txt";

impl NeumannApp {
    /// Asks for a file and shows what it would change; nothing is written yet.
    pub(super) fn import_eq_file(&mut self, index: usize) {
        let Some(path) = self.file_dialog.open() else {
            return;
        };
        let file = file_name(&path);
        let pending = match self.imported_preset(index, &path, &file) {
            Ok((preset, notes)) => {
                let refusal = self.import_refusal(index, &preset);
                PendingImport {
                    file,
                    preset: Some(preset),
                    notes,
                    refusal,
                }
            }
            Err(refusal) => PendingImport {
                file,
                preset: None,
                notes: Vec::new(),
                refusal: Some(refusal),
            },
        };
        let window = &mut self.eq_windows[index];
        window.file_report.clear();
        window.import = Some(pending);
    }

    /// The container after the import, with the notes on what the file holds beyond it.
    fn imported_preset(
        &self,
        index: usize,
        path: &Path,
        file: &str,
    ) -> Result<(EqPreset, Vec<String>), String> {
        let text = read_text(path, file)?;
        let imported = eq_file::parse(&text).map_err(|error| texts::eq_import_error(&error))?;
        let current = self
            .chosen_container(index)
            .and_then(|(container, _)| {
                let speaker = self.eq_speaker(index)?;
                bands_of(&speaker.values, &container)
            })
            .ok_or_else(|| texts::EQ_NOT_REPORTED.get().to_owned())?;
        let preset = eq_file::imported_preset(file, &imported, &current)
            .map_err(|error| texts::eq_import_error(&error))?;
        let mut notes = Vec::new();
        if !imported.ignored.is_empty() {
            notes.push(texts::eq_import_ignored(&imported.ignored));
        }
        Ok((preset, notes))
    }

    /// Why the speaker would refuse the import, checked before anything is written.
    fn import_refusal(&self, index: usize, preset: &EqPreset) -> Option<String> {
        let writes = self.import_writes(index, preset);
        self.eq_refusal(index, &writes)
    }

    fn import_writes(&self, index: usize, preset: &EqPreset) -> Vec<(SscPath, Value)> {
        let Some((container, _)) = self.chosen_container(index) else {
            return Vec::new();
        };
        let Some(speaker) = self.eq_speaker(index) else {
            return Vec::new();
        };
        eq_plan(preset, &speaker.values, &container).writes
    }

    /// The previewed import as one history entry; the speaker's values are compared again, so
    /// only what still differs is written.
    pub(super) fn write_eq_import(&mut self, index: usize, now: f64) {
        let Some(pending) = self.eq_windows[index].import.take() else {
            return;
        };
        let Some(preset) = pending.preset else {
            return;
        };
        let writes = self.import_writes(index, &preset);
        self.write_eq_leaves(index, writes, now);
    }

    /// Asks where to save the chosen container as APO filter lines and says what was left out.
    pub(super) fn export_eq_file(&mut self, index: usize) {
        let Some((suggested, export)) = self.eq_export(index) else {
            return;
        };
        let Some(path) = self.file_dialog.save(&suggested) else {
            return;
        };
        let file = file_name(&path);
        let report = match fs::write(&path, export.text) {
            Ok(()) => texts::eq_export_report(&file, &export.notes),
            Err(error) => texts::eq_file_unwritable(&file, &error.to_string()),
        };
        self.eq_windows[index].file_report = report;
    }

    /// The file name to suggest and the export of the chosen container.
    fn eq_export(&self, index: usize) -> Option<(String, eq_file::Export)> {
        let catalog = Catalog::embedded();
        let (container, _) = self.chosen_container(index)?;
        let speaker = self.eq_speaker(index)?;
        let bands = bands_of(&speaker.values, &container)?;
        let label = catalog
            .eq_containers(speaker.product())
            .into_iter()
            .find(|known| known.path == container)
            .map_or_else(
                || container.to_string(),
                |known| known.label(texts::language()).to_owned(),
            );
        let suggested = safe_file_name(&format!("{} {label}.{FILE_EXTENSION}", speaker.product()));
        Some((suggested, eq_file::export(&bands, &label)))
    }
}

fn read_text(path: &Path, file: &str) -> Result<String, String> {
    let unreadable = |error: std::io::Error| texts::eq_file_unreadable(file, &error.to_string());
    let size = fs::metadata(path).map_err(unreadable)?.len();
    if size > MAX_FILE_BYTES {
        return Err(texts::EQ_FILE_TOO_LARGE.get().to_owned());
    }
    let bytes = fs::read(path).map_err(unreadable)?;
    // Filter files are ASCII; an editor's other code page must not refuse the whole file.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned()
}

fn safe_file_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if FORBIDDEN_IN_FILE_NAMES.contains(&character) {
                return FILE_NAME_REPLACEMENT;
            }
            character
        })
        .collect()
}
