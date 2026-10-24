//! The EQ windows on the app side: which are open, what they show, and the band writes they
//! ask for, which go through the write policy like every other write.

use eframe::egui::{Color32, Context, Id, ViewportId};
use neumann_control_core::catalog::{Catalog, EqContainer, EQ_TYPES};
use neumann_control_core::settings::{CrossoverGuard, EqPreset};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::window_places::WindowKind;
use super::NeumannApp;
use crate::eq::{
    bands_of, container_of, has_eq, input_sums, one_band, output_group, output_key, Band, Leaf,
    ResetKind,
};
use crate::eq_response::{band_curves, curve_db, graph_frequencies};
use crate::history::Entry;
use crate::paths;
use crate::presets::{capture_eq, eq_plan, fitting};
use crate::state::{SpeakerKey, SpeakerState, Status};
use crate::texts;
use crate::ui_kit::eq_palette::{band_color, curve_color, curve_key};
use crate::ui_kit::theme::group_color;
use crate::views::eq_graph::{ComparedCurve, Graph, GraphAction, GraphCurve, GraphPoint, SumCurve};
use crate::views::eq_table::{Limits, Table, TableAction, TableRow};
use crate::views::eq_window::{self, EqAction, EqView, ImportView, PresetDialogView, ResetOffer};
use crate::writes::{self, Intent};

const EQ_VIEWPORT_ID: &str = "eq_window";
const EQ_GRAPH_ID: &str = "eq_graph";
const EQ_TABLE_ID: &str = "eq_table";

/// The output and container a window shows, by place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Drawn {
    output: usize,
    container: usize,
}

impl EqWindow {
    fn drawn(&self) -> Drawn {
        Drawn {
            output: self.output,
            container: self.container,
        }
    }
}

/// An open EQ window and what is chosen in it.
pub(super) struct EqWindow {
    key: SpeakerKey,
    output: usize,
    container: usize,
    band: usize,
    refusal: Option<String>,
    asking_reset: Option<ResetKind>,
    /// Other speakers whose sum is drawn for comparison.
    compared: Vec<SpeakerKey>,
    /// The speakers offered for comparison in the last frame, in the order shown.
    candidates: Vec<SpeakerKey>,
    preset: Option<EqPresetDialog>,
    pub(super) import: Option<PendingImport>,
    /// What the last export did, line by line.
    pub(super) file_report: Vec<String>,
}

/// A filter file read for import, shown before it is written.
pub(super) struct PendingImport {
    pub(super) file: String,
    /// `None` when the file cannot be imported; `refusal` says why.
    pub(super) preset: Option<EqPreset>,
    pub(super) notes: Vec<String>,
    pub(super) refusal: Option<String>,
}

enum EqPresetDialog {
    Save {
        name: String,
    },
    /// `selected` is a place among the presets that fit the container; `deleting` while its
    /// deletion is being confirmed.
    Load {
        selected: Option<usize>,
        deleting: bool,
    },
}

/// One container as the window shows it.
struct Shown<'a> {
    container: &'a EqContainer,
    bands: Option<Vec<Band>>,
    curve: Vec<f64>,
    color: Color32,
}

impl NeumannApp {
    pub(super) fn open_eq(&mut self, key: SpeakerKey) {
        if self.eq_windows.iter().any(|window| window.key == key) {
            return;
        }
        self.eq_windows.push(EqWindow {
            key,
            output: 0,
            container: 0,
            band: 0,
            refusal: None,
            asking_reset: None,
            compared: Vec::new(),
            candidates: Vec::new(),
            preset: None,
            import: None,
            file_report: Vec::new(),
        });
    }

    /// Every open window, each drawn in its own viewport; a window whose speaker is gone closes.
    pub(super) fn show_eq_windows(&mut self, context: &Context) {
        let speakers = &self.speakers;
        self.eq_windows.retain(|window| {
            speakers
                .iter()
                .any(|speaker| speaker.state.key == window.key)
        });
        let now = context.input(|input| input.time);
        let mut closed = Vec::new();
        for window in 0..self.eq_windows.len() {
            // A gesture belongs to the EQ it was drawn for, even when the same frame switched
            // to another one (a field left by clicking another tab).
            let drawn = self.eq_windows[window].drawn();
            let actions = self.show_eq_window(context, window);
            for action in actions {
                if action == EqAction::Close {
                    closed.push(window);
                    continue;
                }
                self.apply_eq_action(window, action, drawn, now);
            }
        }
        for window in closed.into_iter().rev() {
            self.eq_windows.remove(window);
        }
    }

    fn show_eq_window(&mut self, context: &Context, index: usize) -> Vec<EqAction> {
        let catalog = Catalog::embedded();
        let window = &self.eq_windows[index];
        let Some(speaker) = self.speaker_of(&window.key) else {
            return Vec::new();
        };
        let outputs = outputs_of(catalog, speaker.product());
        let output = window.output.min(outputs.len().saturating_sub(1));
        let frequencies = graph_frequencies();
        let shown: Vec<Shown> = outputs
            .get(output)
            .map(|(_, containers)| containers.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|container| {
                let bands = bands_of(&speaker.values, &container.path);
                let curve = curve_db(bands.as_deref().unwrap_or_default(), &frequencies);
                let color = curve_color(&self.settings.eq_display, &curve_key(&container.path));
                Shown {
                    container,
                    bands,
                    curve,
                    color,
                }
            })
            .collect();
        let tagged: Vec<(Option<&str>, &[f64])> = shown
            .iter()
            .map(|shown| (shown.container.input.as_deref(), shown.curve.as_slice()))
            .collect();
        let sums = input_sums(&tagged, frequencies.len());
        let sum_curves: Vec<SumCurve> = sums
            .iter()
            .enumerate()
            .map(|(place, (_, db))| SumCurve {
                db,
                dashed: place > 0,
            })
            .collect();
        let sum_legend: Vec<(String, bool)> = sums
            .iter()
            .enumerate()
            .map(|(place, (input, _))| {
                let name = input
                    .as_deref()
                    .map_or_else(|| texts::EQ_SUM.get().to_owned(), texts::eq_input_sum);
                (name, place > 0)
            })
            .collect();
        let key = shown.first().map(|shown| output_key(shown.container));
        let others: Vec<&SpeakerState> = self
            .speakers
            .iter()
            .map(|speaker| &speaker.state)
            .filter(|other| other.key != window.key && has_eq(catalog, other))
            .filter(|other| {
                key.as_deref()
                    .is_some_and(|key| has_output(catalog, other.product(), key))
            })
            .collect();
        let candidate_keys: Vec<SpeakerKey> =
            others.iter().map(|other| other.key.clone()).collect();
        let candidates: Vec<(String, bool)> = others
            .iter()
            .map(|other| (other.display_name(), window.compared.contains(&other.key)))
            .collect();
        let compared_sums: Vec<(String, Color32, Vec<Vec<f64>>)> = others
            .iter()
            .filter(|other| window.compared.contains(&other.key))
            .filter_map(|other| {
                let sums = output_sums(catalog, other, key.as_deref()?, &frequencies)?;
                Some((other.display_name(), sums))
            })
            .enumerate()
            .map(|(place, (name, sums))| (name, group_color((shown.len() + place) as u8), sums))
            .collect();
        let compared_curves: Vec<ComparedCurve> = compared_sums
            .iter()
            .flat_map(|(_, color, sums)| sums.iter().map(|db| ComparedCurve { color: *color, db }))
            .collect();
        let legend: Vec<(String, Color32)> = compared_sums
            .iter()
            .map(|(name, color, _)| (name.clone(), *color))
            .collect();
        let chosen = window.container.min(shown.len().saturating_sub(1));
        let selected = shown.get(chosen);
        let bands = selected.and_then(|shown| shown.bands.clone());
        let editable = selected.is_some_and(|shown| {
            let gain = Leaf::Gain.path(&shown.container.path);
            bands.is_some() && writes::can_write(speaker, catalog, &gain)
        });
        let curves: Vec<GraphCurve> = shown
            .iter()
            .map(|shown| GraphCurve {
                color: shown.color,
                db: &shown.curve,
            })
            .collect();
        let dark = context.theme() == eframe::egui::Theme::Dark;
        let display = &self.settings.eq_display;
        let points: Vec<GraphPoint> = bands
            .iter()
            .flatten()
            .enumerate()
            .map(|(place, band)| GraphPoint {
                color: band_color(display, place, dark),
                frequency: band.frequency,
                gain: band.gain,
                boost: band.boost,
                q: band.q,
                enabled: band.enabled,
            })
            .collect();
        let limits = limits_of(catalog, selected.map(|shown| &shown.container.path));
        let headers = headers_of(catalog, selected.map(|shown| &shown.container.path));
        let rows: Option<Vec<TableRow>> = bands.as_ref().map(|bands| {
            bands
                .iter()
                .map(|band| TableRow {
                    enabled: band.enabled,
                    kind: band.kind.clone(),
                    frequency: band.frequency,
                    q: band.q,
                    gain: band.gain,
                    boost: band.boost,
                })
                .collect()
        });
        let id = Id::unique(EQ_VIEWPORT_ID).with(&window.key);
        let own_curves = bands
            .as_deref()
            .map(|bands| band_curves(bands, &frequencies))
            .unwrap_or_default();
        let band_colors: Vec<Color32> = points.iter().map(|point| point.color).collect();
        let graph = Graph {
            id: id.with(EQ_GRAPH_ID).with((window.output, chosen)),
            frequencies: &frequencies,
            curves: &curves,
            sums: &sum_curves,
            compared: &compared_curves,
            band_curves: &own_curves,
            band_curve_mode: display.band_curves,
            points: &points,
            selected: window.band,
            editable,
            gain_range: limits.gain.clone(),
            q_range: limits.q.clone(),
        };
        let table = rows.as_ref().map(|rows| Table {
            id: id.with(EQ_TABLE_ID).with((window.output, chosen)),
            rows,
            limits: &limits,
            headers: &headers,
            editable,
            selected: window.band,
            colors: &band_colors,
        });
        let output_names: Vec<String> = outputs.iter().map(|(name, _)| name.clone()).collect();
        let tabs: Vec<(String, Color32)> = shown
            .iter()
            .map(|shown| {
                (
                    shown.container.label(texts::language()).to_owned(),
                    shown.color,
                )
            })
            .collect();
        let unreachable = speaker.status != Status::Connected;
        let notice = unreachable
            .then(|| texts::UNREACHABLE.get().to_owned())
            .or_else(|| window.refusal.clone());
        let undo = self
            .eq_undo(&window.key)
            .and_then(|id| self.history.history.entry(id))
            .map(|entry| self.row(entry).text);
        let question = self
            .history
            .question
            .filter(|_| self.eq_question_window() == Some(index))
            .and_then(|id| self.history.history.entry(id))
            .map(|entry| self.changed_lines(entry));
        let crossovers_open = self.settings.desk.crossovers == CrossoverGuard::Open;
        let reshapable =
            selected.filter(|shown| editable && (crossovers_open || !shown.container.crossover));
        let band_count = bands.as_ref().map_or(0, Vec::len);
        let fitting_presets = fitting(&self.settings.eq_presets, band_count);
        let preset_names: Vec<String> = fitting_presets
            .iter()
            .map(|place| {
                let preset = &self.settings.eq_presets[*place];
                texts::eq_preset_entry(&preset.name, preset.bands)
            })
            .collect();
        let preset_changes: Vec<String> = match (&window.preset, reshapable) {
            (
                Some(EqPresetDialog::Load {
                    selected: Some(place),
                    ..
                }),
                Some(shown),
            ) => fitting_presets
                .get(*place)
                .map(|place| {
                    let preset = &self.settings.eq_presets[*place];
                    let plan = eq_plan(preset, &speaker.values, &shown.container.path);
                    plan.bands
                        .iter()
                        .map(|band| texts::eq_preset_band(band + 1))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let deleting_name = match &window.preset {
            Some(EqPresetDialog::Load {
                selected: Some(place),
                deleting: true,
            }) => fitting_presets
                .get(*place)
                .map(|place| self.settings.eq_presets[*place].name.as_str()),
            _ => None,
        };
        let preset_dialog = match &window.preset {
            Some(EqPresetDialog::Save { name }) => Some(PresetDialogView::Save {
                name,
                replacing: eq_preset_taken(&self.settings.eq_presets, name, band_count),
            }),
            Some(EqPresetDialog::Load { selected, .. }) => Some(PresetDialogView::Load {
                names: &preset_names,
                selected: *selected,
                changes: &preset_changes,
                crossover: reshapable.is_some_and(|shown| shown.container.crossover),
                deleting: deleting_name,
            }),
            None => None,
        };
        let import_changes: Vec<String> = match (&window.import, reshapable) {
            (Some(pending), Some(shown)) => pending
                .preset
                .as_ref()
                .map(|preset| {
                    eq_plan(preset, &speaker.values, &shown.container.path)
                        .bands
                        .iter()
                        .map(|band| texts::eq_preset_band(band + 1))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let import = window.import.as_ref().map(|pending| ImportView {
            file: &pending.file,
            changes: &import_changes,
            notes: &pending.notes,
            refusal: pending.refusal.as_deref(),
            crossover: reshapable.is_some_and(|shown| shown.container.crossover),
        });
        let reset = reshapable.map(|shown| ResetOffer {
            container: shown.container.label(texts::language()).to_owned(),
            asking: window.asking_reset,
            crossover: shown.container.crossover,
        });
        let viewport = ViewportId::from_hash_of(id);
        let opening = self.opening(context, viewport, WindowKind::Eq, &window.key);
        let view = EqView {
            viewport,
            opening,
            title: texts::eq_window_title(&speaker.display_name()),
            outputs: &output_names,
            output,
            tabs: &tabs,
            sums: &sum_legend,
            container: chosen,
            graph,
            table,
            notice,
            reset,
            undo,
            question,
            candidates: &candidates,
            compared: &legend,
            presets: reshapable.is_some(),
            preset_dialog,
            exportable: selected.is_some_and(|shown| shown.bands.is_some()),
            import,
            file_report: &window.file_report,
        };
        let actions = eq_window::show(context, &view);
        let (viewport, key) = (view.viewport, self.eq_windows[index].key.clone());
        self.remember_place(context, viewport, WindowKind::Eq, &key);
        self.eq_windows[index].candidates = candidate_keys;
        actions
    }

    /// The newest entry the EQ window of `key` can undo: open, and only EQ writes to it.
    fn eq_undo(&self, key: &SpeakerKey) -> Option<u64> {
        let catalog = Catalog::embedded();
        self.history
            .history
            .newest_open_where(|entry| is_eq_entry_of(entry, key, catalog))
    }

    /// The EQ window that asks the pending "changed since" question, if it is about an EQ
    /// entry of a speaker whose window is open.
    pub(super) fn eq_question_window(&self) -> Option<usize> {
        let catalog = Catalog::embedded();
        let entry = self
            .history
            .question
            .and_then(|id| self.history.history.entry(id))?;
        self.eq_windows
            .iter()
            .position(|window| is_eq_entry_of(entry, &window.key, catalog))
    }

    /// The speaker of the EQ window at `index`.
    pub(super) fn eq_speaker(&self, index: usize) -> Option<&SpeakerState> {
        self.speaker_of(&self.eq_windows[index].key)
    }

    pub(super) fn speaker_of(&self, key: &SpeakerKey) -> Option<&SpeakerState> {
        self.speakers
            .iter()
            .map(|speaker| &speaker.state)
            .find(|state| state.key == *key)
    }

    fn apply_eq_action(&mut self, index: usize, action: EqAction, drawn: Drawn, now: f64) {
        let window = &mut self.eq_windows[index];
        match action {
            EqAction::Close => {}
            EqAction::Output(output) => {
                (window.output, window.container, window.band) = (output, 0, 0);
                window.asking_reset = None;
            }
            EqAction::Container(container) => {
                (window.container, window.band) = (container, 0);
                window.asking_reset = None;
            }
            EqAction::ResetAsk(kind) => window.asking_reset = Some(kind),
            EqAction::ResetCancel => window.asking_reset = None,
            EqAction::ResetConfirm => {
                if let Some(kind) = window.asking_reset.take() {
                    self.reset_container(index, kind, now);
                }
            }
            EqAction::Undo => {
                let key = window.key.clone();
                if let Some(id) = self.eq_undo(&key) {
                    self.request_take_back(id);
                }
            }
            EqAction::PresetSave => {
                window.preset = Some(EqPresetDialog::Save {
                    name: String::new(),
                });
            }
            EqAction::PresetLoad => {
                window.preset = Some(EqPresetDialog::Load {
                    selected: None,
                    deleting: false,
                });
            }
            EqAction::PresetName(typed) => {
                if let Some(EqPresetDialog::Save { name }) = &mut window.preset {
                    *name = typed;
                }
            }
            EqAction::PresetSelect(place) => {
                window.preset = Some(EqPresetDialog::Load {
                    selected: Some(place),
                    deleting: false,
                });
            }
            EqAction::PresetClose => window.preset = None,
            EqAction::PresetConfirmSave => self.save_eq_preset(index),
            EqAction::PresetWrite => self.write_eq_preset(index, now),
            EqAction::PresetDelete | EqAction::PresetCancelDelete => {
                let asking = action == EqAction::PresetDelete;
                if let Some(EqPresetDialog::Load { deleting, .. }) = &mut window.preset {
                    *deleting = asking;
                }
            }
            EqAction::PresetConfirmDelete => self.delete_eq_preset(index),
            EqAction::Import => self.import_eq_file(index),
            EqAction::ImportWrite => self.write_eq_import(index, now),
            EqAction::ImportCancel => window.import = None,
            EqAction::Export => self.export_eq_file(index),
            EqAction::Compare(place, on) => {
                let Some(key) = window.candidates.get(place).cloned() else {
                    return;
                };
                window.compared.retain(|compared| *compared != key);
                if on {
                    window.compared.push(key);
                }
            }
            EqAction::QuestionConfirm => self.answer_take_back(true),
            EqAction::QuestionCancel => self.answer_take_back(false),
            EqAction::Graph(GraphAction::Select(band))
            | EqAction::Table(TableAction::Select(band)) => window.band = band,
            EqAction::Graph(GraphAction::Move {
                band,
                frequency,
                boost,
            }) => {
                let writes = [(Leaf::Frequency, frequency), (Leaf::Boost, boost)];
                for (leaf, value) in writes {
                    if let Some(value) = value {
                        self.write_band(index, drawn, band, leaf, Value::from(value), now);
                    }
                }
            }
            EqAction::Graph(GraphAction::Q { band, q }) => {
                self.write_band(index, drawn, band, Leaf::Q, Value::from(q), now);
            }
            EqAction::Table(TableAction::Write { band, leaf, value }) => {
                self.write_band(index, drawn, band, leaf, value, now);
            }
        }
    }

    /// The chosen container's path and its band count, once the speaker reported it.
    pub(super) fn chosen_container(&self, index: usize) -> Option<(SscPath, usize)> {
        self.container_at(index, self.eq_windows[index].drawn())
    }

    /// The path and band count of the container at `place`, once the speaker reported it.
    fn container_at(&self, index: usize, place: Drawn) -> Option<(SscPath, usize)> {
        let catalog = Catalog::embedded();
        let window = &self.eq_windows[index];
        let speaker = self.speaker_of(&window.key)?;
        let outputs = outputs_of(catalog, speaker.product());
        let container = outputs
            .get(place.output)
            .and_then(|(_, containers)| containers.get(place.container))?;
        let length = speaker
            .values
            .get(&Leaf::Gain.path(&container.path))
            .and_then(Value::as_array)
            .map(Vec::len)?;
        Some((container.path.clone(), length))
    }

    fn save_eq_preset(&mut self, index: usize) {
        let window = &self.eq_windows[index];
        let Some(EqPresetDialog::Save { name }) = &window.preset else {
            return;
        };
        let name = name.trim().to_owned();
        let preset = self.chosen_container(index).and_then(|(container, _)| {
            let speaker = self.eq_speaker(index)?;
            capture_eq(&name, &speaker.values, &container)
        });
        if let Some(preset) = preset {
            // A preset of another band count is another preset, though named alike: the load
            // dialog of this EQ does not even show it.
            let presets = &mut self.settings.eq_presets;
            presets.retain(|known| known.name != preset.name || known.bands != preset.bands);
            presets.push(preset);
            self.save_settings();
        }
        self.eq_windows[index].preset = None;
    }

    /// The preset place in the settings that the load dialog's selection stands for.
    fn selected_eq_preset(&self, index: usize) -> Option<usize> {
        let Some(EqPresetDialog::Load {
            selected: Some(place),
            ..
        }) = &self.eq_windows[index].preset
        else {
            return None;
        };
        let (_, bands) = self.chosen_container(index)?;
        fitting(&self.settings.eq_presets, bands)
            .get(*place)
            .copied()
    }

    /// Every differing leaf whole, in this frame: one history entry.
    fn write_eq_preset(&mut self, index: usize, now: f64) {
        let plan = self.selected_eq_preset(index).and_then(|place| {
            let (container, _) = self.chosen_container(index)?;
            let speaker = self.eq_speaker(index)?;
            Some(eq_plan(
                &self.settings.eq_presets[place],
                &speaker.values,
                &container,
            ))
        });
        self.eq_windows[index].preset = None;
        self.write_eq_leaves(index, plan.map(|plan| plan.writes).unwrap_or_default(), now);
    }

    /// Whole leaves in this frame (one history entry), all or none: every write is checked
    /// before the first is sent, so a refused leaf never leaves the others half written.
    pub(super) fn write_eq_leaves(
        &mut self,
        index: usize,
        writes: Vec<(SscPath, Value)>,
        now: f64,
    ) {
        if let Some(refusal) = self.eq_refusal(index, &writes) {
            self.eq_windows[index].refusal = Some(refusal);
            return;
        }
        for (path, value) in writes {
            if let Err((label, reason)) = self.send_eq_write(index, &path, value, now) {
                self.eq_windows[index].refusal = Some(texts::setting_refusal(&label, &reason));
                return;
            }
        }
    }

    /// Why the write policy would refuse one of `writes`, as the window says it.
    pub(super) fn eq_refusal(&self, index: usize, writes: &[(SscPath, Value)]) -> Option<String> {
        let catalog = Catalog::embedded();
        let speaker = self.eq_speaker(index)?;
        writes.iter().find_map(|(path, value)| {
            let refusal = writes::check(speaker, catalog, path, value).err()?;
            let label = leaf_label(catalog, path);
            Some(texts::setting_refusal(
                &label,
                &texts::describe_refusal(&refusal),
            ))
        })
    }

    fn delete_eq_preset(&mut self, index: usize) {
        if let Some(place) = self.selected_eq_preset(index) {
            self.settings.eq_presets.remove(place);
            self.save_settings();
        }
        self.eq_windows[index].preset = Some(EqPresetDialog::Load {
            selected: None,
            deleting: false,
        });
    }

    /// Every band of the chosen container at once, in this frame, so the history files it as
    /// one entry.
    fn reset_container(&mut self, index: usize, kind: ResetKind, now: f64) {
        for leaf in kind.leaves() {
            let Some((path, length)) = self.chosen_leaf(index, *leaf) else {
                return;
            };
            let value = kind.value(*leaf, length);
            if let Err((label, reason)) = self.send_eq_write(index, &path, value, now) {
                self.eq_windows[index].refusal = Some(texts::setting_refusal(&label, &reason));
                return;
            }
        }
    }

    /// The chosen container's `leaf` and its band count, once the speaker reported it.
    fn chosen_leaf(&self, index: usize, leaf: Leaf) -> Option<(SscPath, usize)> {
        self.leaf_at(index, self.eq_windows[index].drawn(), leaf)
    }

    /// `leaf` of the container at `place` and its band count, once the speaker reported it.
    fn leaf_at(&self, index: usize, place: Drawn, leaf: Leaf) -> Option<(SscPath, usize)> {
        let (container, _) = self.container_at(index, place)?;
        let speaker = self.eq_speaker(index)?;
        let path = leaf.path(&container);
        let length = speaker
            .values
            .get(&path)
            .and_then(Value::as_array)
            .map(Vec::len)?;
        Some((path, length))
    }

    /// One leaf of one band of the container at `place`, `null` elsewhere; a refusal is shown in
    /// the window.
    fn write_band(
        &mut self,
        index: usize,
        place: Drawn,
        band: usize,
        leaf: Leaf,
        value: Value,
        now: f64,
    ) {
        let Some((path, length)) = self.leaf_at(index, place, leaf) else {
            return;
        };
        let Some(value) = one_band(length, band, value) else {
            return;
        };
        let refusal = self
            .send_eq_write(index, &path, value, now)
            .err()
            .map(|(label, reason)| texts::eq_refusal(band + 1, &label, &reason));
        self.eq_windows[index].refusal = refusal;
    }

    /// One EQ write through the write policy; a refusal comes back as the leaf's name and the
    /// reason, for the window to word.
    fn send_eq_write(
        &mut self,
        index: usize,
        path: &SscPath,
        value: Value,
        now: f64,
    ) -> Result<(), (String, String)> {
        let catalog = Catalog::embedded();
        let key = &self.eq_windows[index].key;
        let Some(speaker) = self
            .speakers
            .iter_mut()
            .find(|speaker| speaker.state.key == *key)
        else {
            return Ok(());
        };
        let intent = Intent::Write {
            path: path.clone(),
            value,
        };
        match speaker.state.try_request(intent, catalog, now) {
            Ok(command) => {
                speaker.handle.send(command);
                Ok(())
            }
            Err(refusal) => Err((leaf_label(catalog, path), texts::describe_refusal(&refusal))),
        }
    }
}

/// A leaf's name as the window shows it.
fn leaf_label(catalog: &Catalog, path: &SscPath) -> String {
    paths::catalog_entry(catalog, path).map_or_else(
        || path.to_string(),
        |entry| entry.label(texts::language()).to_owned(),
    )
}

/// Whether every change of `entry` is an EQ write to the speaker `key`.
fn is_eq_entry_of(entry: &Entry, key: &SpeakerKey, catalog: &Catalog) -> bool {
    entry
        .changes
        .iter()
        .all(|change| change.speaker == *key && container_of(catalog, &change.path).is_some())
}

fn has_output(catalog: &Catalog, product: &str, key: &str) -> bool {
    catalog
        .eq_containers(product)
        .iter()
        .any(|container| output_key(container) == key)
}

/// The sums of the speaker's EQs on output `key`, one per input where it has crossovers; `None`
/// while it reported none of them.
fn output_sums(
    catalog: &Catalog,
    speaker: &SpeakerState,
    key: &str,
    frequencies: &[f64],
) -> Option<Vec<Vec<f64>>> {
    let curves: Vec<(Option<&str>, Vec<f64>)> = catalog
        .eq_containers(speaker.product())
        .into_iter()
        .filter(|container| output_key(container) == key)
        .filter_map(|container| {
            let bands = bands_of(&speaker.values, &container.path)?;
            Some((container.input.as_deref(), curve_db(&bands, frequencies)))
        })
        .collect();
    if curves.is_empty() {
        return None;
    }
    let tagged: Vec<(Option<&str>, &[f64])> = curves
        .iter()
        .map(|(input, curve)| (*input, curve.as_slice()))
        .collect();
    let sums = input_sums(&tagged, frequencies.len());
    Some(sums.into_iter().map(|(_, db)| db).collect())
}

/// The speaker's EQ containers by output (`out`, `out1`, `out2`), in catalog order.
fn outputs_of<'a>(catalog: &'a Catalog, product: &str) -> Vec<(String, Vec<&'a EqContainer>)> {
    let mut outputs: Vec<(String, Vec<&EqContainer>)> = Vec::new();
    for container in catalog.eq_containers(product) {
        let name = texts::eq_output(output_group(container)).to_owned();
        match outputs.iter_mut().find(|(known, _)| *known == name) {
            Some((_, containers)) => containers.push(container),
            None => outputs.push((name, vec![container])),
        }
    }
    outputs
}

/// The catalog's limits for the container's leaves; without a container, nothing is allowed.
fn limits_of(catalog: &Catalog, container: Option<&SscPath>) -> Limits {
    let entry = |leaf: Leaf| {
        container.and_then(|container| paths::catalog_entry(catalog, &leaf.path(container)))
    };
    let range = |leaf: Leaf| {
        let min = entry(leaf).and_then(|entry| entry.min).unwrap_or(0.0);
        let max = entry(leaf).and_then(|entry| entry.max).unwrap_or(0.0);
        min..=max
    };
    let unit = |leaf: Leaf| entry(leaf).and_then(|entry| entry.unit.clone());
    Limits {
        frequency: range(Leaf::Frequency),
        q: range(Leaf::Q),
        gain: range(Leaf::Gain),
        types: EQ_TYPES.iter().map(|kind| (*kind).to_owned()).collect(),
        frequency_unit: unit(Leaf::Frequency),
        gain_unit: unit(Leaf::Gain),
    }
}

/// Column titles in `Leaf::ALL` order, from the catalog in the current language.
fn headers_of(catalog: &Catalog, container: Option<&SscPath>) -> [String; 6] {
    Leaf::ALL.map(|leaf| {
        container
            .and_then(|container| paths::catalog_entry(catalog, &leaf.path(container)))
            .map_or_else(
                || leaf.name().to_owned(),
                |entry| entry.label(texts::language()).to_owned(),
            )
    })
}

/// Whether saving `name` for a container of `bands` bands would replace a preset.
fn eq_preset_taken(presets: &[EqPreset], name: &str, bands: usize) -> bool {
    let name = name.trim();
    presets
        .iter()
        .any(|preset| preset.name == name && preset.bands == bands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Change, ClockTime};
    use serde_json::json;

    fn key(serial: &str) -> SpeakerKey {
        SpeakerKey::Serial(serial.to_owned())
    }

    fn entry(changes: &[(&SpeakerKey, &str)]) -> Entry {
        let changes = changes
            .iter()
            .map(|(speaker, path)| Change {
                speaker: (*speaker).clone(),
                path: crate::paths::path(path),
                before: Some(json!(0.0)),
                after: json!(1.0),
                taken_back: false,
                write: None,
                refused: false,
            })
            .collect();
        Entry {
            id: 0,
            clock: ClockTime {
                hour: 12,
                minute: 0,
                second: 0,
            },
            changes,
            takes_back: None,
        }
    }

    #[test]
    fn an_eq_entry_belongs_to_its_speaker_only() {
        let catalog = Catalog::embedded();
        let (left, right) = (key("A"), key("B"));
        let flattened = entry(&[
            (&left, "audio/out/eq2/gain"),
            (&left, "audio/out/eq2/boost"),
        ]);
        assert!(is_eq_entry_of(&flattened, &left, catalog));
        assert!(!is_eq_entry_of(&flattened, &right, catalog));
        let muted = entry(&[(&left, "audio/out/mute")]);
        assert!(!is_eq_entry_of(&muted, &left, catalog));
        let mixed = entry(&[(&left, "audio/out/eq2/gain"), (&left, "audio/out/level")]);
        assert!(!is_eq_entry_of(&mixed, &left, catalog));
        let both = entry(&[
            (&left, "audio/out/eq2/gain"),
            (&right, "audio/out/eq2/gain"),
        ]);
        assert!(!is_eq_entry_of(&both, &left, catalog));
    }
}
