//! Settings presets: which settings a speaker's preset can hold, taking one, and what loading
//! one onto a speaker would write. The writes themselves go through the write policy.

use neumann_control_core::catalog::{Catalog, Entry};
use std::collections::BTreeMap;

use neumann_control_core::settings::{EqPreset, SettingsPreset};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::eq::Leaf;
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

/// Loading an EQ preset: the leaves to write whole, and the bands that change (for the preview).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EqPlan {
    pub writes: Vec<(SscPath, Value)>,
    /// Zero-based.
    pub bands: Vec<usize>,
}

/// The container's six leaves as reported; `None` until the speaker reported all of them with
/// the same band count.
pub fn capture_eq(
    name: &str,
    values: &BTreeMap<SscPath, Value>,
    container: &SscPath,
) -> Option<EqPreset> {
    let mut leaves = BTreeMap::new();
    let mut bands = None;
    for leaf in Leaf::ALL {
        let array = values.get(&leaf.path(container))?.as_array()?;
        if *bands.get_or_insert(array.len()) != array.len() {
            return None;
        }
        leaves.insert(leaf.name().to_owned(), Value::Array(array.clone()));
    }
    Some(EqPreset {
        name: name.to_owned(),
        bands: bands?,
        leaves,
    })
}

/// The places of the presets that fit a container of `bands` bands.
pub fn fitting(presets: &[EqPreset], bands: usize) -> Vec<usize> {
    presets
        .iter()
        .enumerate()
        .filter(|(_, preset)| preset.bands == bands)
        .map(|(place, _)| place)
        .collect()
}

/// The leaves whose arrays differ from what the speaker reports, written whole.
pub fn eq_plan(
    preset: &EqPreset,
    values: &BTreeMap<SscPath, Value>,
    container: &SscPath,
) -> EqPlan {
    let mut plan = EqPlan::default();
    for leaf in Leaf::ALL {
        let Some(wanted) = preset.leaves.get(leaf.name()).and_then(Value::as_array) else {
            continue;
        };
        let path = leaf.path(container);
        let held = values.get(&path).and_then(Value::as_array);
        let differing: Vec<usize> = (0..wanted.len())
            .filter(|band| held.and_then(|held| held.get(*band)) != wanted.get(*band))
            .collect();
        if differing.is_empty() {
            continue;
        }
        for band in differing {
            if !plan.bands.contains(&band) {
                plan.bands.push(band);
            }
        }
        plan.writes.push((path, Value::Array(wanted.clone())));
    }
    plan.bands.sort_unstable();
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::speaker::{Event, Identity};
    use serde_json::json;
    use std::net::SocketAddrV6;

    use crate::eq::Leaf;
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

    fn eq_values(gain: Value) -> BTreeMap<SscPath, Value> {
        let container = paths::path("audio/out/eq2");
        Leaf::ALL
            .iter()
            .map(|leaf| {
                let value = match leaf {
                    Leaf::Gain => gain.clone(),
                    Leaf::Enabled => json!([true, false]),
                    Leaf::Type => json!(["PARAMETRIC", "NOTCH"]),
                    _ => json!([1.0, 2.0]),
                };
                (leaf.path(&container), value)
            })
            .collect()
    }

    #[test]
    fn an_eq_preset_holds_all_six_leaves_of_a_container() {
        let container = paths::path("audio/out/eq2");
        let preset = capture_eq("Warm", &eq_values(json!([0.0, 3.0])), &container).unwrap();
        assert_eq!(preset.bands, 2);
        assert_eq!(preset.leaves.len(), 6);
        assert_eq!(preset.leaves["gain"], json!([0.0, 3.0]));
        let mut missing = eq_values(json!([0.0, 3.0]));
        missing.remove(&Leaf::Q.path(&container));
        assert!(capture_eq("Warm", &missing, &container).is_none());
    }

    #[test]
    fn loading_an_eq_preset_writes_only_differing_leaves() {
        let container = paths::path("audio/out/eq2");
        let preset = capture_eq("Warm", &eq_values(json!([0.0, 3.0])), &container).unwrap();
        let now = eq_values(json!([0.0, -2.0]));
        let plan = eq_plan(&preset, &now, &container);
        assert_eq!(plan.bands, vec![1]);
        assert_eq!(
            plan.writes,
            vec![(Leaf::Gain.path(&container), json!([0.0, 3.0]))]
        );
        assert!(eq_plan(&preset, &eq_values(json!([0.0, 3.0])), &container)
            .writes
            .is_empty());
    }

    #[test]
    fn only_presets_with_the_same_band_count_fit() {
        let preset = |bands| EqPreset {
            name: format!("{bands}"),
            bands,
            leaves: BTreeMap::new(),
        };
        let presets = [preset(10), preset(20), preset(10)];
        assert_eq!(fitting(&presets, 10), vec![0, 2]);
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
