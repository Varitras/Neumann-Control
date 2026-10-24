//! "Komplett scannen" on the app side: a finished walk becomes two report files next to the
//! settings (next to the exe), and the speaker notes where they went.

use std::fs;
use std::path::{Path, PathBuf};

use neumann_control_core::catalog::Catalog;
use neumann_control_core::schema_walk::LeafReport;
use serde_json::Value;

use super::NeumannApp;
use crate::paths;
use crate::scan_report::{file_stem, json_report, redacted, text_report, ScanHeader};
use crate::state::{FullScanState, ScanFailure, SpeakerState};

const JSON_EXTENSION: &str = "json";
const TEXT_EXTENSION: &str = "txt";
/// The second report of a name is "-2", as the first is the plain name.
const FIRST_SUFFIX: u32 = 2;

impl NeumannApp {
    /// Writes the report of every speaker whose walk just finished.
    pub(super) fn write_scan_reports(&mut self) {
        let directory = self
            .settings_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for speaker in &mut self.speakers {
            let state = &mut speaker.state;
            // Taken out and put back unless read: a running scan must stay running.
            let reports = match std::mem::take(&mut state.full_scan) {
                FullScanState::Read(reports) => reports,
                other => {
                    state.full_scan = other;
                    continue;
                }
            };
            state.full_scan = match write_report(&directory, state, &reports) {
                Ok(path) => FullScanState::Written(path.display().to_string()),
                Err(error) => {
                    log::warn!("scan report not written ({})", error.kind());
                    FullScanState::Failed(ScanFailure::Writing)
                }
            };
        }
    }
}

/// Both files; returns the JSON file's path.
fn write_report(
    directory: &Path,
    speaker: &SpeakerState,
    reports: &[LeafReport],
) -> std::io::Result<PathBuf> {
    let catalog = Catalog::embedded();
    let model = speaker.product().to_owned();
    let firmware = speaker
        .identity
        .as_ref()
        .map(|identity| identity.version.clone())
        .unwrap_or_default();
    let (serial, announced) = speaker
        .identity
        .as_ref()
        .map(|identity| (identity.serial.clone(), identity.name.clone()))
        .unwrap_or_default();
    let name = speaker
        .values
        .get(&paths::path(paths::DEVICE_NAME))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let now = super::local_now();
    let time = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let compact_time = time.replace(['-', ':'], "").replace(' ', "-");
    let header = ScanHeader {
        model: &model,
        firmware: &firmware,
        time: &time,
    };
    let known = [serial.as_str(), name.as_str(), announced.as_str()];
    let reports = redacted(reports, &known);
    let stem = free_stem(directory, &file_stem(&model, &firmware, &compact_time));
    let json_path = directory.join(format!("{stem}.{JSON_EXTENSION}"));
    let json = serde_json::to_string_pretty(&json_report(&header, &reports, catalog))
        .map_err(std::io::Error::other)?;
    fs::write(&json_path, json)?;
    let text = text_report(&header, &reports, catalog);
    fs::write(directory.join(format!("{stem}.{TEXT_EXTENSION}")), text)?;
    Ok(json_path)
}

/// `stem`, or with `-2`, `-3` … when a report of that name is already there: the name holds
/// the time to the second only, and nothing that tells two speakers of one model apart.
fn free_stem(directory: &Path, stem: &str) -> String {
    let taken = |candidate: &str| {
        [JSON_EXTENSION, TEXT_EXTENSION]
            .iter()
            .any(|extension| directory.join(format!("{candidate}.{extension}")).exists())
    };
    if !taken(stem) {
        return stem.to_owned();
    }
    (FIRST_SUFFIX..)
        .map(|number| format!("{stem}-{number}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two speakers of one model and firmware whose walks end in the same second: the second
    /// report must not replace the first.
    #[test]
    fn a_second_report_in_the_same_second_gets_its_own_name() {
        let directory = tempfile::tempdir().unwrap();
        let stem = "scan-KH-750-2_1_2-20261020-190405";
        assert_eq!(free_stem(directory.path(), stem), stem);
        fs::write(
            directory.path().join(format!("{stem}.{JSON_EXTENSION}")),
            "",
        )
        .unwrap();
        assert_eq!(free_stem(directory.path(), stem), format!("{stem}-2"));
        fs::write(
            directory.path().join(format!("{stem}-2.{TEXT_EXTENSION}")),
            "",
        )
        .unwrap();
        assert_eq!(free_stem(directory.path(), stem), format!("{stem}-3"));
    }
}
