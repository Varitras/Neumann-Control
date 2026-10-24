//! The settings presets on the app side: the open dialog, saving into the settings file, and
//! writing a loaded preset in one frame, so the history files it as one entry.

use eframe::egui::Ui;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::NeumannApp;
use crate::display::shown_value;
use crate::paths;
use crate::presets::{capture, chosen_by_default, plan, presettable};
use crate::state::SpeakerKey;
use crate::texts;
use crate::views::preset_dialog::{self, LoadAction, LoadView, SaveAction, SaveRow};
use crate::writes::Intent;

/// Shown for a value the speaker has not reported.
const UNKNOWN_VALUE: &str = "?";

pub(super) enum PresetDialog {
    Save {
        key: SpeakerKey,
        name: String,
        chosen: Vec<SscPath>,
    },
    Load {
        key: SpeakerKey,
        selected: Option<usize>,
        /// While the selected preset's deletion is being confirmed.
        deleting: bool,
    },
}

impl NeumannApp {
    pub(super) fn open_preset_save(&mut self, key: SpeakerKey) {
        let catalog = Catalog::embedded();
        let Some(speaker) = self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == key)
        else {
            return;
        };
        let chosen = presettable(catalog, &speaker.state)
            .into_iter()
            .filter(|entry| chosen_by_default(entry))
            .map(|entry| entry.path.clone())
            .collect();
        self.preset_dialog = Some(PresetDialog::Save {
            key,
            name: String::new(),
            chosen,
        });
    }

    pub(super) fn open_preset_load(&mut self, key: SpeakerKey) {
        self.preset_dialog = Some(PresetDialog::Load {
            key,
            selected: None,
            deleting: false,
        });
    }

    pub(super) fn show_preset_dialog(&mut self, ui: &mut Ui) {
        let now = ui.input(|input| input.time);
        match self.preset_dialog.take() {
            None => {}
            Some(PresetDialog::Save { key, name, chosen }) => {
                self.preset_dialog = self.show_save(ui, key, name, chosen);
            }
            Some(PresetDialog::Load {
                key,
                selected,
                deleting,
            }) => {
                self.preset_dialog = self.show_load(ui, key, selected, deleting, now);
            }
        }
    }

    /// Returns the dialog as it stays open, or `None` when it closed.
    fn show_save(
        &mut self,
        ui: &mut Ui,
        key: SpeakerKey,
        mut name: String,
        mut chosen: Vec<SscPath>,
    ) -> Option<PresetDialog> {
        let catalog = Catalog::embedded();
        let speaker = &self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == key)?
            .state;
        let entries = presettable(catalog, speaker);
        let rows: Vec<SaveRow> = entries
            .iter()
            .map(|entry| SaveRow {
                label: entry.label(texts::language()).to_owned(),
                value: speaker.values.get(&entry.path).map_or_else(
                    || UNKNOWN_VALUE.to_owned(),
                    |value| shown_value(entry, value),
                ),
                chosen: chosen.contains(&entry.path),
            })
            .collect();
        let product = speaker.product().to_owned();
        let replacing = self
            .settings
            .presets
            .iter()
            .any(|preset| preset.name == name.trim() && preset.product == product);
        let title = texts::SAVE_PRESET_TITLE.get();
        match preset_dialog::save(ui, title, &rows, &mut name, replacing) {
            SaveAction::None => {}
            SaveAction::Toggle(place, on) => {
                let path = entries[place].path.clone();
                chosen.retain(|known| *known != path);
                if on {
                    chosen.push(path);
                }
            }
            SaveAction::Save => {
                let preset = capture(name.trim(), speaker, &chosen);
                // Listed with their model: one of the same name for another model is another.
                let presets = &mut self.settings.presets;
                presets
                    .retain(|known| known.name != preset.name || known.product != preset.product);
                presets.push(preset);
                self.save_settings();
                return None;
            }
            SaveAction::Cancel => return None,
        }
        Some(PresetDialog::Save { key, name, chosen })
    }

    fn show_load(
        &mut self,
        ui: &mut Ui,
        key: SpeakerKey,
        mut selected: Option<usize>,
        mut deleting: bool,
        now: f64,
    ) -> Option<PresetDialog> {
        let catalog = Catalog::embedded();
        let index = self
            .speakers
            .iter()
            .position(|speaker| speaker.state.key == key)?;
        let names: Vec<String> = self
            .settings
            .presets
            .iter()
            .map(|preset| texts::preset_entry(&preset.name, &preset.product))
            .collect();
        let speaker = &self.speakers[index].state;
        let loading = selected
            .and_then(|place| self.settings.presets.get(place))
            .map(|preset| plan(preset, speaker, catalog))
            .unwrap_or_default();
        let shown = |path: &SscPath, value: Option<&Value>| {
            let entry = paths::catalog_entry(catalog, path);
            match (entry, value) {
                (Some(entry), Some(value)) => shown_value(entry, value),
                _ => UNKNOWN_VALUE.to_owned(),
            }
        };
        let changes: Vec<String> = loading
            .changes
            .iter()
            .map(|change| {
                let label = paths::catalog_entry(catalog, &change.path).map_or_else(
                    || change.path.to_string(),
                    |entry| entry.label(texts::language()).to_owned(),
                );
                let current = shown(&change.path, change.current.as_ref());
                let preset = shown(&change.path, Some(&change.preset));
                texts::preset_change(&label, &current, &preset)
            })
            .collect();
        let view = LoadView {
            title: texts::LOAD_PRESET_TITLE.get(),
            presets: &names,
            selected,
            changes: &changes,
            skipped: &loading.skipped,
            warning: None,
            deleting: selected
                .filter(|_| deleting)
                .and_then(|place| self.settings.presets.get(place))
                .map(|preset| preset.name.as_str()),
        };
        match preset_dialog::load(ui, &view) {
            LoadAction::None => {}
            LoadAction::Select(place) => {
                selected = Some(place);
                deleting = false;
            }
            LoadAction::Write => {
                let speaker = &mut self.speakers[index];
                for change in loading.changes {
                    let intent = Intent::restoring(&change.path, change.preset);
                    if let Some(command) = speaker.state.request(intent, catalog, now) {
                        speaker.handle.send(command);
                    }
                }
                return None;
            }
            LoadAction::Delete => deleting = true,
            LoadAction::CancelDelete => deleting = false,
            LoadAction::ConfirmDelete => {
                deleting = false;
                if let Some(place) = selected.take() {
                    self.settings.presets.remove(place);
                    self.save_settings();
                }
            }
            LoadAction::Close => return None,
        }
        Some(PresetDialog::Load {
            key,
            selected,
            deleting,
        })
    }
}
