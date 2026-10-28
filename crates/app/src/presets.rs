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
        .filter(|entry| is_presettable(entry, product))
        .collect()
}

/// Both for what a preset takes and what loading one writes: a stored preset may hold more.
fn is_presettable(entry: &Entry, product: &str) -> bool {
    let restorable = !NOT_PRESETTABLE.contains(&entry.path.to_string().as_str());
    entry.is_writable(product) && !entry.bands && restorable
}

/// Checked when the save dialog opens: everything but the speaker's own name.
pub fn chosen_by_default(entry: &Entry) -> bool {
    entry.path != paths::path(paths::DEVICE_NAME)
}

/// The chosen settings as the speaker will hold them once its writes are through; one it has
/// not reported is left out.
pub fn capture(name: &str, speaker: &SpeakerState, chosen: &[SscPath]) -> SettingsPreset {
    let values = chosen
        .iter()
        .filter_map(|path| {
            let value = speaker.latest(path)?;
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
        let Some(entry) = entry.filter(|entry| is_presettable(entry, product)) else {
            plan.skipped.push(text.clone());
            continue;
        };
        let current = speaker.latest(&entry.path).cloned();
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
    /// Each band's value that changes, in band order, for the comparison.
    pub differences: Vec<EqDifference>,
}

/// One value of one band a preset changes: as the speaker has it, and as written.
#[derive(Clone, Debug, PartialEq)]
pub struct EqDifference {
    /// Zero-based.
    pub band: usize,
    pub leaf: Leaf,
    /// `None` when the speaker has not reported it.
    pub now: Option<Value>,
    pub written: Value,
}

/// The container's six leaves in `values` (the speaker's latest, writes on their way
/// included); `None` until all of them are there with the same band count.
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

/// The places of the presets that fit a container of `bands` bands: all six leaves, each that
/// long, since the speaker refuses a shorter array after the other leaves were written.
pub fn fitting(presets: &[EqPreset], bands: usize) -> Vec<usize> {
    presets
        .iter()
        .enumerate()
        .filter(|(_, preset)| preset.bands == bands && is_whole(preset))
        .map(|(place, _)| place)
        .collect()
}

fn is_whole(preset: &EqPreset) -> bool {
    Leaf::ALL.iter().all(|leaf| {
        let array = preset.leaves.get(leaf.name()).and_then(Value::as_array);
        array.is_some_and(|array| array.len() == preset.bands)
    })
}

/// The leaves whose arrays differ from `values` (the speaker's latest, writes on their way
/// included), written whole.
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
            .filter(|band| {
                let held = held.and_then(|held| held.get(*band));
                !held.is_some_and(|held| same_value(held, &wanted[*band], None))
            })
            .collect();
        if differing.is_empty() {
            continue;
        }
        for band in differing {
            if !plan.bands.contains(&band) {
                plan.bands.push(band);
            }
            plan.differences.push(EqDifference {
                band,
                leaf,
                now: held.and_then(|held| held.get(band)).cloned(),
                written: wanted[band].clone(),
            });
        }
        plan.writes.push((path, Value::Array(wanted.clone())));
    }
    plan.bands.sort_unstable();
    plan.differences.sort_by_key(|difference| difference.band);
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

    fn whole_preset(bands: usize) -> EqPreset {
        let leaves = Leaf::ALL
            .iter()
            .map(|leaf| (leaf.name().to_owned(), json!(vec![1.0; bands])))
            .collect();
        EqPreset {
            name: format!("{bands}"),
            bands,
            leaves,
        }
    }

    #[test]
    fn only_presets_with_the_same_band_count_fit() {
        let presets = [whole_preset(10), whole_preset(20), whole_preset(10)];
        assert_eq!(fitting(&presets, 10), vec![0, 2]);
    }

    /// A hand-edited or damaged settings file: a leaf missing or of another length would be
    /// written only in part, so such a preset is not offered.
    #[test]
    fn an_eq_preset_with_a_missing_or_short_leaf_does_not_fit() {
        let mut missing = whole_preset(2);
        missing.leaves.remove(Leaf::Q.name());
        let mut short = whole_preset(2);
        short
            .leaves
            .insert(Leaf::Gain.name().to_owned(), json!([0.0]));
        assert_eq!(fitting(&[missing, short, whole_preset(2)], 2), vec![2]);
    }

    /// The speaker may report `0` for a written `0.0`; that band has not changed.
    #[test]
    fn a_band_reported_as_a_whole_number_equals_the_same_decimal() {
        let container = paths::path("audio/out/eq2");
        let preset = capture_eq("Warm", &eq_values(json!([0.0, 3.0])), &container).unwrap();
        let plan = eq_plan(&preset, &eq_values(json!([0, 3])), &container);
        assert_eq!(plan, EqPlan::default());
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

    /// A write still on its way is what the speaker will hold: loading a preset that has the
    /// value reported before must still write it, and saving takes the new value.
    #[test]
    fn a_preset_counts_a_write_still_on_its_way() {
        let catalog = Catalog::embedded();
        let mut monitor = speaker("KH 120 II", &[(paths::MUTE, json!(false))]);
        monitor.record_written(&paths::path(paths::MUTE), &json!(true));
        let preset = SettingsPreset {
            name: "Open".to_owned(),
            product: "KH 120 II".to_owned(),
            values: [(paths::MUTE.to_owned(), json!(false))].into(),
        };
        let plan = plan(&preset, &monitor, catalog);
        assert_eq!(
            plan.changes,
            vec![PresetChange {
                path: paths::path(paths::MUTE),
                current: Some(json!(true)),
                preset: json!(false),
            }]
        );
        let saved = capture("Now", &monitor, &[paths::path(paths::MUTE)]);
        assert_eq!(saved.values, [(paths::MUTE.to_owned(), json!(true))].into());
    }

    /// A settings file written by hand may hold identify; loading it must not flash the logo.
    #[test]
    fn identify_in_a_stored_preset_is_not_written() {
        let catalog = Catalog::embedded();
        let preset = SettingsPreset {
            name: "Old".to_owned(),
            product: "KH 120 II".to_owned(),
            values: [(paths::IDENTIFY.to_owned(), json!(true))].into(),
        };
        let monitor = speaker("KH 120 II", &[(paths::IDENTIFY, json!(false))]);
        let plan = plan(&preset, &monitor, catalog);
        assert_eq!(plan.changes, Vec::new());
        assert_eq!(plan.skipped, vec![paths::IDENTIFY]);
    }
}
