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
use crate::state::{FullScanState, SpeakerState};

const JSON_EXTENSION: &str = "json";
const TEXT_EXTENSION: &str = "txt";

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
                Err(error) => FullScanState::Failed(error.to_string()),
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
    let serial = speaker
        .identity
        .as_ref()
        .map(|identity| identity.serial.clone())
        .unwrap_or_default();
    let name = speaker
        .values
        .get(&paths::path(paths::DEVICE_NAME))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
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
    let reports = redacted(reports, &[serial.as_str(), name.as_str()]);
    let stem = file_stem(&model, &firmware, &compact_time);
    let json_path = directory.join(format!("{stem}.{JSON_EXTENSION}"));
    let json = serde_json::to_string_pretty(&json_report(&header, &reports, catalog))
        .map_err(std::io::Error::other)?;
    fs::write(&json_path, json)?;
    let text = text_report(&header, &reports, catalog);
    fs::write(directory.join(format!("{stem}.{TEXT_EXTENSION}")), text)?;
    Ok(json_path)
}
