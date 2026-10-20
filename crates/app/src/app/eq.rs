//! The EQ windows on the app side: which are open, what they show, and the band writes they
//! ask for, which go through the write policy like every other write.

use eframe::egui::{Color32, Context, Id, ViewportId};
use neumann_control_core::catalog::{Catalog, EqContainer, EQ_TYPES};
use neumann_control_core::settings::CrossoverGuard;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::NeumannApp;
use crate::eq::{bands_of, container_of, flat, has_eq, one_band, Band, Leaf, FLATTENING_LEAVES};
use crate::eq_response::{curve_db, graph_frequencies};
use crate::history::Entry;
use crate::paths;
use crate::state::{SpeakerKey, SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::group_color;
use crate::views::eq_graph::{ComparedCurve, Graph, GraphAction, GraphCurve, GraphPoint};
use crate::views::eq_table::{Limits, Table, TableAction, TableRow};
use crate::views::eq_window::{self, EqAction, EqView, ResetOffer};
use crate::writes::{self, Intent};

const EQ_VIEWPORT_ID: &str = "eq_window";
const EQ_GRAPH_ID: &str = "eq_graph";
const EQ_TABLE_ID: &str = "eq_table";

/// An open EQ window and what is chosen in it.
pub(super) struct EqWindow {
    key: SpeakerKey,
    output: usize,
    container: usize,
    band: usize,
    refusal: Option<String>,
    asking_reset: bool,
    /// Other speakers whose sum is drawn for comparison.
    compared: Vec<SpeakerKey>,
    /// The speakers offered for comparison in the last frame, in the order shown.
    candidates: Vec<SpeakerKey>,
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
            asking_reset: false,
            compared: Vec::new(),
            candidates: Vec::new(),
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
            let actions = self.show_eq_window(context, window);
            for action in actions {
                if action == EqAction::Close {
                    closed.push(window);
                    continue;
                }
                self.apply_eq_action(window, action, now);
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
            .enumerate()
            .map(|(place, container)| {
                let bands = bands_of(&speaker.values, &container.path);
                let curve = curve_db(bands.as_deref().unwrap_or_default(), &frequencies);
                let color = group_color(place as u8);
                Shown {
                    container,
                    bands,
                    curve,
                    color,
                }
            })
            .collect();
        let sum: Vec<f64> = (0..frequencies.len())
            .map(|point| shown.iter().map(|container| container.curve[point]).sum())
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
        let compared_sums: Vec<(String, Color32, Vec<f64>)> = others
            .iter()
            .filter(|other| window.compared.contains(&other.key))
            .filter_map(|other| {
                let sum = output_sum(catalog, other, key.as_deref()?, &frequencies)?;
                Some((other.display_name(), sum))
            })
            .enumerate()
            .map(|(place, (name, sum))| (name, group_color((shown.len() + place) as u8), sum))
            .collect();
        let compared_curves: Vec<ComparedCurve> = compared_sums
            .iter()
            .map(|(_, color, db)| ComparedCurve { color: *color, db })
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
        let points: Vec<GraphPoint> = bands
            .iter()
            .flatten()
            .map(|band| GraphPoint {
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
        let id = Id::new(EQ_VIEWPORT_ID).with(&window.key);
        let color = selected.map_or(Color32::GRAY, |shown| shown.color);
        let graph = Graph {
            id: id.with(EQ_GRAPH_ID).with(chosen),
            frequencies: &frequencies,
            curves: &curves,
            sum: &sum,
            compared: &compared_curves,
            points: &points,
            color,
            selected: window.band,
            editable,
            gain_range: limits.gain.clone(),
            q_range: limits.q.clone(),
        };
        let table = rows.as_ref().map(|rows| Table {
            id: id.with(EQ_TABLE_ID).with(chosen),
            rows,
            limits: &limits,
            headers: &headers,
            editable,
            selected: window.band,
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
        let reset = selected
            .filter(|shown| editable && (crossovers_open || !shown.container.crossover))
            .map(|shown| ResetOffer {
                container: shown.container.label(texts::language()).to_owned(),
                asking: window.asking_reset,
                crossover: shown.container.crossover,
            });
        let view = EqView {
            viewport: ViewportId::from_hash_of(id),
            title: texts::eq_window_title(&speaker.display_name()),
            outputs: &output_names,
            output,
            tabs: &tabs,
            container: chosen,
            graph,
            table,
            notice,
            reset,
            undo,
            question,
            candidates: &candidates,
            compared: &legend,
        };
        let actions = eq_window::show(context, &view);
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

    fn speaker_of(&self, key: &SpeakerKey) -> Option<&SpeakerState> {
        self.speakers
            .iter()
            .map(|speaker| &speaker.state)
            .find(|state| state.key == *key)
    }

    fn apply_eq_action(&mut self, index: usize, action: EqAction, now: f64) {
        let window = &mut self.eq_windows[index];
        match action {
            EqAction::Close => {}
            EqAction::Output(output) => {
                (window.output, window.container, window.band) = (output, 0, 0);
                window.asking_reset = false;
            }
            EqAction::Container(container) => {
                (window.container, window.band) = (container, 0);
                window.asking_reset = false;
            }
            EqAction::ResetAsk => window.asking_reset = true,
            EqAction::ResetCancel => window.asking_reset = false,
            EqAction::ResetConfirm => {
                window.asking_reset = false;
                self.flatten(index, now);
            }
            EqAction::Undo => {
                let key = window.key.clone();
                if let Some(id) = self.eq_undo(&key) {
                    self.request_take_back(id);
                }
            }
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
                        self.write_band(index, band, leaf, Value::from(value), now);
                    }
                }
            }
            EqAction::Graph(GraphAction::Q { band, q }) => {
                self.write_band(index, band, Leaf::Q, Value::from(q), now);
            }
            EqAction::Table(TableAction::Write { band, leaf, value }) => {
                self.write_band(index, band, leaf, value, now);
            }
        }
    }

    /// `gain` and `boost` of every band of the chosen container to 0, both in this frame, so
    /// the history files them as one entry.
    fn flatten(&mut self, index: usize, now: f64) {
        for leaf in FLATTENING_LEAVES {
            let Some((path, length)) = self.chosen_leaf(index, leaf) else {
                return;
            };
            if let Err((label, reason)) = self.send_eq_write(index, &path, flat(length), now) {
                self.eq_windows[index].refusal = Some(texts::eq_leaf_refusal(&label, &reason));
                return;
            }
        }
    }

    /// The chosen container's `leaf` and its band count, once the speaker reported it.
    fn chosen_leaf(&self, index: usize, leaf: Leaf) -> Option<(SscPath, usize)> {
        let catalog = Catalog::embedded();
        let window = &self.eq_windows[index];
        let speaker = self.speaker_of(&window.key)?;
        let outputs = outputs_of(catalog, speaker.product());
        let container = outputs
            .get(window.output)
            .and_then(|(_, containers)| containers.get(window.container))?;
        let path = leaf.path(&container.path);
        let length = speaker
            .values
            .get(&path)
            .and_then(Value::as_array)
            .map(Vec::len)?;
        Some((path, length))
    }

    /// One leaf of one band, `null` elsewhere; a refusal is shown in the window.
    fn write_band(&mut self, index: usize, band: usize, leaf: Leaf, value: Value, now: f64) {
        let Some((path, length)) = self.chosen_leaf(index, leaf) else {
            return;
        };
        let refusal = self
            .send_eq_write(index, &path, one_band(length, band, value), now)
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
            Err(refusal) => {
                let label = paths::catalog_entry(catalog, path).map_or_else(
                    || path.to_string(),
                    |entry| entry.label(texts::language()).to_owned(),
                );
                Err((label, texts::describe_refusal(&refusal)))
            }
        }
    }
}

/// Whether every change of `entry` is an EQ write to the speaker `key`.
fn is_eq_entry_of(entry: &Entry, key: &SpeakerKey, catalog: &Catalog) -> bool {
    entry
        .changes
        .iter()
        .all(|change| change.speaker == *key && container_of(catalog, &change.path).is_some())
}

/// The output a container belongs to: `out`, `out1` or `out2`.
fn output_key(container: &EqContainer) -> String {
    container
        .path
        .segments()
        .get(1)
        .cloned()
        .unwrap_or_default()
}

fn has_output(catalog: &Catalog, product: &str, key: &str) -> bool {
    catalog
        .eq_containers(product)
        .iter()
        .any(|container| output_key(container) == key)
}

/// The sum of the speaker's EQs on output `key`; `None` while it reported none of them.
fn output_sum(
    catalog: &Catalog,
    speaker: &SpeakerState,
    key: &str,
    frequencies: &[f64],
) -> Option<Vec<f64>> {
    let curves: Vec<Vec<f64>> = catalog
        .eq_containers(speaker.product())
        .into_iter()
        .filter(|container| output_key(container) == key)
        .filter_map(|container| bands_of(&speaker.values, &container.path))
        .map(|bands| curve_db(&bands, frequencies))
        .collect();
    if curves.is_empty() {
        return None;
    }
    let sum = (0..frequencies.len())
        .map(|point| curves.iter().map(|curve| curve[point]).sum())
        .collect();
    Some(sum)
}

/// The speaker's EQ containers by output (`out`, `out1`, `out2`), in catalog order.
fn outputs_of<'a>(catalog: &'a Catalog, product: &str) -> Vec<(String, Vec<&'a EqContainer>)> {
    let mut outputs: Vec<(String, Vec<&EqContainer>)> = Vec::new();
    for container in catalog.eq_containers(product) {
        let name = output_name(&output_key(container));
        match outputs.iter_mut().find(|(known, _)| *known == name) {
            Some((_, containers)) => containers.push(container),
            None => outputs.push((name, vec![container])),
        }
    }
    outputs
}

fn output_name(output: &str) -> String {
    let text = match output {
        "out1" => texts::EQ_OUTPUT_A,
        "out2" => texts::EQ_OUTPUT_B,
        _ => texts::EQ_OUTPUT_SUBWOOFER,
    };
    text.get().to_owned()
}

/// The catalog's limits for the container's leaves; without a container, nothing is allowed.
fn limits_of(catalog: &Catalog, container: Option<&SscPath>) -> Limits {
    let range = |leaf: Leaf| {
        let entry =
            container.and_then(|container| paths::catalog_entry(catalog, &leaf.path(container)));
        let min = entry.and_then(|entry| entry.min).unwrap_or(0.0);
        let max = entry.and_then(|entry| entry.max).unwrap_or(0.0);
        min..=max
    };
    Limits {
        frequency: range(Leaf::Frequency),
        q: range(Leaf::Q),
        gain: range(Leaf::Gain),
        types: EQ_TYPES.iter().map(|kind| (*kind).to_owned()).collect(),
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
