//! Settings presets: which settings a speaker's preset can hold, taking one, and what loading
//! one onto a speaker would write. The writes themselves go through the write policy.

use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::settings::SettingsPreset;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::history::same_value;
use crate::paths;
use crate::state::SpeakerState;

/// Settings that are not a state to restore: identify only flashes the logo.
const NOT_PRESETTABLE: &[&str] = &[paths::IDENTIFY];

/// What one write of a loaded preset changes.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetChange {
    pub path: SscPath,
    /// `None` while the speaker has not reported the value.
    pub current: Option<Value>,
    pub preset: Value,
}

/// Loading a preset onto a speaker: the writes, and the paths it may not write (another model,
/// or a path the catalog does not know).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetPlan {
    pub changes: Vec<PresetChange>,
    pub skipped: Vec<String>,
}

/// The settings a preset of this speaker can hold: writable on its model, not an EQ band.
pub fn presettable<'a>(catalog: &'a Catalog, speaker: &SpeakerState) -> Vec<&'a Entry> {
    let product = speaker.product();
    catalog
        .entries_for(product)
        .into_iter()
        .filter(|entry| entry.is_writable(product) && !entry.bands)
        .filter(|entry| !NOT_PRESETTABLE.contains(&entry.path.to_string().as_str()))
        .collect()
}

/// Checked when the save dialog opens: everything but the speaker's own name.
pub fn chosen_by_default(entry: &Entry) -> bool {
    entry.path != paths::path(paths::DEVICE_NAME)
}

/// The chosen settings as the speaker reports them now; one it has not reported is left out.
pub fn capture(name: &str, speaker: &SpeakerState, chosen: &[SscPath]) -> SettingsPreset {
    let values = chosen
        .iter()
        .filter_map(|path| {
            let value = speaker.values.get(path)?;
            Some((path.to_string(), value.clone()))
        })
        .collect();
    SettingsPreset {
        name: name.to_owned(),
        product: speaker.product().to_owned(),
        values,
    }
}

/// The writes loading `preset` needs: only paths this speaker may write and whose value differs.
pub fn plan(preset: &SettingsPreset, speaker: &SpeakerState, catalog: &Catalog) -> PresetPlan {
    let product = speaker.product();
    let mut plan = PresetPlan::default();
    for (text, value) in &preset.values {
        let entry = SscPath::parse(text).ok().and_then(|path| {
            catalog
                .entries_for(product)
                .into_iter()
                .find(|entry| entry.path == path)
        });
        let Some(entry) = entry.filter(|entry| entry.is_writable(product) && !entry.bands) else {
            plan.skipped.push(text.clone());
            continue;
        };
        let current = speaker.values.get(&entry.path).cloned();
        let unchanged = current
            .as_ref()
            .is_some_and(|current| same_value(current, value, entry.step));
        if !unchanged {
            plan.changes.push(PresetChange {
                path: entry.path.clone(),
                current,
                preset: value.clone(),
            });
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::speaker::{Event, Identity};
    use serde_json::json;
    use std::net::SocketAddrV6;

    use crate::state::SpeakerKey;

    fn speaker(product: &str, values: &[(&str, Value)]) -> SpeakerState {
        let address: SocketAddrV6 = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: product.to_owned(),
            version: "1".to_owned(),
            serial: "SIM".to_owned(),
            name: String::new(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        for (path, value) in values {
            let path = paths::path(path);
            speaker.apply(
                Event::Value {
                    path,
                    value: value.clone(),
                },
                0.0,
            );
        }
        speaker
    }

    #[test]
    fn a_preset_holds_writable_settings_but_not_identify_or_eq_bands() {
        let catalog = Catalog::embedded();
        let monitor = speaker("KH 120 II", &[]);
        let paths: Vec<String> = presettable(catalog, &monitor)
            .iter()
            .map(|entry| entry.path.to_string())
            .collect();
        assert!(paths.contains(&"audio/out/level".to_owned()));
        assert!(paths.contains(&"device/name".to_owned()));
        assert!(!paths.contains(&paths::IDENTIFY.to_owned()));
        assert!(!paths.iter().any(|path| path.contains("/eq")));
        assert!(!paths.contains(&"ui/control_mode".to_owned()));
    }

    #[test]
    fn capture_takes_the_chosen_reported_values() {
        let monitor = speaker(
            "KH 120 II",
            &[
                ("audio/out/level", json!(85.0)),
                ("audio/out/delay", json!(10)),
            ],
        );
        let chosen = [paths::path("audio/out/level"), paths::path("device/name")];
        let preset = capture("Night", &monitor, &chosen);
        assert_eq!(preset.product, "KH 120 II");
        assert_eq!(
            preset.values,
            [("audio/out/level".to_owned(), json!(85.0))].into()
        );
    }

    #[test]
    fn loading_writes_only_differing_writable_paths_and_names_the_rest() {
        let catalog = Catalog::embedded();
        let preset = SettingsPreset {
            name: "Sub".to_owned(),
            product: "KH 750".to_owned(),
            values: [
                ("audio/out/level".to_owned(), json!(90.0)),
                ("audio/out/delay".to_owned(), json!(10)),
                ("audio/out1/level".to_owned(), json!(95.0)),
                ("no/such/path".to_owned(), json!(1)),
            ]
            .into(),
        };
        let monitor = speaker(
            "KH 120 II",
            &[
                ("audio/out/level", json!(85.0)),
                ("audio/out/delay", json!(10)),
            ],
        );
        let plan = plan(&preset, &monitor, catalog);
        assert_eq!(
            plan.changes,
            vec![PresetChange {
                path: paths::path("audio/out/level"),
                current: Some(json!(85.0)),
                preset: json!(90.0),
            }]
        );
        assert_eq!(plan.skipped, vec!["audio/out1/level", "no/such/path"]);
    }
}
