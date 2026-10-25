//! The session's history on the app side: the writes of each frame become one entry, and an
//! entry is taken back by writing its values before through the normal write policy.

use eframe::egui::Ui;

use super::NeumannApp;
use serde_json::Value;

use crate::display::{conversion_of, converted};
use crate::eq::{container_of, output_group, several_outputs};
use crate::history::{changed_since, is_open, Change, ClockTime, Entry, History};
use crate::paths;
use crate::state::{SpeakerKey, SpeakerState, Status};
use crate::texts;
use crate::views::history::{
    self as history_view, HistoryAction, HistoryRow, QuestionAction, UNDO_SHORTCUT,
};
use crate::writes::Intent;
use neumann_control_core::catalog::{Catalog, Conversion};
use neumann_control_core::speaker::Command;

/// What the last take-back could not write, for the history window.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeBackNotice {
    /// The names of the speakers left out (gone, unreachable, or refused).
    pub skipped: Vec<String>,
}

/// The history and the take-back waiting for its turn or for the user's answer.
#[derive(Debug, Default)]
pub(super) struct HistoryState {
    pub history: History,
    /// The entry to take back at the end of this frame, and whether the user confirmed it.
    pub request: Option<(u64, bool)>,
    /// An entry whose values changed since; the user is asked before it is taken back.
    pub question: Option<u64>,
    pub notice: Option<TakeBackNotice>,
    pub open: bool,
    /// Take-back writes the speaker has not answered yet.
    awaited: Vec<AwaitedTakeBack>,
}

/// A change marked taken back when its write was sent; refused, it is open again.
#[derive(Debug)]
struct AwaitedTakeBack {
    speaker: SpeakerKey,
    write: u64,
    entry: u64,
    change: usize,
}

/// How a change reads: whose, which setting, and its unit.
struct Described {
    speaker: String,
    setting: String,
    unit: Option<String>,
    /// The panel's conversion of the setting (a delay in ms or m), so the line reads the same.
    conversion: Option<Conversion>,
    /// For an EQ band write: the band it touched; its values are that band's entries.
    band: Option<usize>,
    /// An EQ write of every band (a reset to flat).
    all_bands: bool,
}

impl Described {
    /// The part of `value` the line shows: the band's entry of an EQ array, one entry for
    /// all bands holding the same, else all of it.
    fn shown<'a>(&self, value: &'a Value) -> Option<&'a Value> {
        if let Some(band) = self.band {
            return value.as_array().and_then(|bands| bands.get(band));
        }
        let bands = value.as_array().filter(|_| self.all_bands);
        let first = bands.and_then(|bands| bands.first());
        let alike = bands.is_some_and(|bands| bands.iter().all(|band| Some(band) == first));
        if alike {
            return first;
        }
        Some(value)
    }

    /// `value` as the panel shows it.
    fn text(&self, value: &Value) -> String {
        let Some(shown) = self.shown(value) else {
            return texts::UNKNOWN_VALUE.to_owned();
        };
        match (self.conversion, shown.as_f64()) {
            (Some(conversion), Some(number)) => converted(conversion, number),
            _ => texts::history_value(shown, self.unit.as_deref()),
        }
    }
}

impl NeumannApp {
    pub(super) fn open_history(&mut self) {
        self.history.open = true;
    }

    /// Ctrl+Z: takes back the newest open entry. Not while a text field has the keyboard (it
    /// has its own undo) or a dialog waits for an answer.
    pub(super) fn undo_shortcut(&mut self, ui: &mut Ui) {
        let typing = ui.ctx().memory(|memory| memory.focused().is_some());
        if typing || self.dialog_open() {
            return;
        }
        if !ui.input_mut(|input| input.consume_shortcut(&UNDO_SHORTCUT)) {
            return;
        }
        if let Some(id) = self.history.history.newest_open() {
            self.request_take_back(id);
        }
    }

    fn dialog_open(&self) -> bool {
        self.settings_open
            || self.removal.is_some()
            || self.add_address.is_some()
            || self.scan_choice.is_some()
            || self.group_draft.is_some()
            || self.history.question.is_some()
            || self.preset_dialog.is_some()
    }

    pub(super) fn show_history(&mut self, ui: &mut Ui) {
        if !self.history.open {
            return;
        }
        let notice = self
            .history
            .notice
            .as_ref()
            .map(|notice| texts::take_back_skipped(&notice.skipped));
        let count = self.history.history.len();
        let row_at = |place| {
            self.history
                .history
                .newest(place)
                .map(|entry| self.row(entry))
        };
        match history_view::show(ui, count, &row_at, notice.as_deref()) {
            HistoryAction::None => {}
            HistoryAction::Close => {
                self.history.open = false;
                self.history.notice = None;
            }
            HistoryAction::TakeBack(id) => self.request_take_back(id),
        }
    }

    pub(super) fn show_take_back_question(&mut self, ui: &mut Ui) {
        let Some(id) = self.history.question else {
            return;
        };
        // An EQ entry is asked about in its speaker's EQ window, where the user undid it.
        if self.eq_question_window().is_some() {
            return;
        }
        let Some(entry) = self.history.history.entry(id) else {
            self.history.question = None;
            return;
        };
        let lines = self.changed_lines(entry);
        match history_view::question(ui, &lines) {
            QuestionAction::None => {}
            QuestionAction::Confirm => self.answer_take_back(true),
            QuestionAction::Cancel => self.answer_take_back(false),
        }
    }

    pub(super) fn row(&self, entry: &Entry) -> HistoryRow {
        let lines: Vec<String> = entry
            .changes
            .iter()
            .map(|change| self.change_line(change))
            .collect();
        let text = match lines.as_slice() {
            [line] => line.clone(),
            _ => {
                let first = &entry.changes[0].path;
                let shared = entry.changes.iter().all(|change| change.path == *first);
                let setting = shared.then(|| self.describe(&entry.changes[0]).setting);
                texts::history_changes(setting.as_deref(), entry.changes.len())
            }
        };
        let text = match entry
            .takes_back
            .and_then(|id| self.history.history.entry(id))
        {
            Some(original) => texts::history_take_back(&clock_text(original.clock), &text),
            None => text,
        };
        let open = entry.changes.iter().any(is_open);
        let all_taken_back = entry.changes.iter().all(|change| change.taken_back);
        let all_refused = entry.changes.iter().all(|change| change.refused);
        let reason = match (all_taken_back, all_refused) {
            (true, _) => texts::TAKEN_BACK.get(),
            (false, true) => texts::NOT_WRITTEN.get(),
            (false, false) => texts::BEFORE_UNKNOWN.get(),
        };
        HistoryRow {
            id: entry.id,
            time: clock_text(entry.clock),
            text,
            details: if lines.len() > 1 { lines } else { Vec::new() },
            open,
            reason: (!open).then(|| reason.to_owned()),
        }
    }

    fn change_line(&self, change: &Change) -> String {
        let described = self.describe(change);
        let before = change.before.as_ref().map_or_else(
            || texts::UNKNOWN_VALUE.to_owned(),
            |before| described.text(before),
        );
        let after = described.text(&change.after);
        texts::history_change(&described.speaker, &described.setting, &before, &after)
    }

    /// The open changes whose speakers now hold another value than the written one.
    pub(super) fn changed_lines(&self, entry: &Entry) -> Vec<String> {
        let catalog = Catalog::embedded();
        let mut lines = Vec::new();
        for change in entry.changes.iter().filter(|change| is_open(change)) {
            let latest = self.latest(change);
            let step = paths::catalog_entry(catalog, &change.path).and_then(|entry| entry.step);
            let Some(now) = latest.filter(|_| changed_since(change, latest, step)) else {
                continue;
            };
            let described = self.describe(change);
            lines.push(texts::changed_now(
                &described.speaker,
                &described.setting,
                &described.text(now),
                &described.text(&change.after),
            ));
        }
        lines
    }

    fn latest(&self, change: &Change) -> Option<&serde_json::Value> {
        let speaker = self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == change.speaker);
        speaker.and_then(|speaker| speaker.state.latest(&change.path))
    }

    fn describe(&self, change: &Change) -> Described {
        let state = self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == change.speaker)
            .map(|speaker| &speaker.state);
        let speaker = state.map_or_else(
            || texts::REMOVED_SPEAKER.get().to_owned(),
            SpeakerState::display_name,
        );
        let catalog = Catalog::embedded();
        let entry = paths::catalog_entry(catalog, &change.path);
        let language = texts::language();
        let label = entry.map_or_else(
            || change.path.to_string(),
            |entry| entry.label(language).to_owned(),
        );
        let given: Vec<usize> = change
            .after
            .as_array()
            .map(|bands| {
                let written = bands.iter().enumerate().filter(|(_, band)| !band.is_null());
                written.map(|(band, _)| band).collect()
            })
            .unwrap_or_default();
        let container = container_of(catalog, &change.path);
        let product = state.map_or("", SpeakerState::product);
        let container_name = container.map(|container| {
            let name = container.label(language);
            match several_outputs(catalog, product) {
                true => texts::eq_on_output(texts::eq_output(output_group(container)), name),
                false => name.to_owned(),
            }
        });
        let (setting, band, all_bands) = match (container_name, given.as_slice()) {
            (Some(container), [band]) => {
                let setting = texts::eq_change_setting(&container, band + 1, &label);
                (setting, Some(*band), false)
            }
            (Some(container), [_, _, ..]) => {
                let setting = texts::eq_all_bands_setting(&container, &label);
                (setting, None, true)
            }
            _ => (label, None, false),
        };
        Described {
            speaker,
            setting,
            unit: entry.and_then(|entry| entry.unit.clone()),
            conversion: entry.and_then(conversion_of),
            band,
            all_bands,
        }
    }

    pub fn history(&self) -> &History {
        &self.history.history
    }

    pub fn request_take_back(&mut self, id: u64) {
        self.history.request = Some((id, false));
    }

    pub fn take_back_question(&self) -> Option<u64> {
        self.history.question
    }

    /// The answer to "changed since": `true` takes the entry back anyway.
    pub fn answer_take_back(&mut self, confirmed: bool) {
        let question = self.history.question.take();
        if confirmed {
            self.history.request = question.map(|id| (id, true));
        }
    }

    pub fn take_back_notice(&self) -> Option<&TakeBackNotice> {
        self.history.notice.as_ref()
    }

    /// Last in the frame: runs a requested take-back, then files everything this frame wrote,
    /// the take-back's own writes included, as one entry.
    pub(super) fn finish_history_frame(&mut self, now: f64) {
        self.settle_refused_writes();
        let mut takes_back = None;
        if let Some((id, confirmed)) = self.history.request.take() {
            takes_back = self.take_back(id, confirmed, now);
        }
        let changes: Vec<Change> = self
            .speakers
            .iter_mut()
            .flat_map(|speaker| {
                let key = speaker.state.key.clone();
                speaker
                    .state
                    .written
                    .drain(..)
                    .map(move |written| Change {
                        speaker: key.clone(),
                        path: written.path,
                        before: written.before,
                        after: written.after,
                        taken_back: false,
                        write: Some(written.write),
                        refused: false,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.history.history.push(clock_now(), changes, takes_back);
    }

    /// Closes the changes the speaker refused or never got, and opens again the ones their
    /// take-back was; forgets the take-backs answered.
    fn settle_refused_writes(&mut self) {
        for speaker in &mut self.speakers {
            let refused = std::mem::take(&mut speaker.state.refused_writes);
            let key = &speaker.state.key;
            for write in &refused {
                self.history.history.mark_refused(key, *write);
            }
            let awaited = self.history.awaited.iter();
            for awaited in awaited.filter(|awaited| awaited.speaker == *key) {
                if refused.contains(&awaited.write) {
                    self.history.history.reopen(awaited.entry, awaited.change);
                }
            }
        }
        let speakers = &self.speakers;
        self.history.awaited.retain(|awaited| {
            speakers.iter().any(|speaker| {
                speaker.state.key == awaited.speaker && speaker.state.awaits_write(awaited.write)
            })
        });
    }

    /// Writes the open changes' values before; returns the entry when it was taken back (in
    /// part or whole), `None` when it waits for the user's answer or is gone.
    fn take_back(&mut self, id: u64, confirmed: bool, now: f64) -> Option<u64> {
        let catalog = Catalog::embedded();
        let entry = self.history.history.entry(id)?.clone();
        if !confirmed && self.changed_since(&entry, catalog) {
            self.history.question = Some(id);
            return None;
        }
        let mut skipped = Vec::new();
        for (index, change) in entry.changes.iter().enumerate() {
            if !is_open(change) {
                continue;
            }
            let speaker = self
                .speakers
                .iter_mut()
                .find(|speaker| speaker.state.key == change.speaker);
            let Some(speaker) = speaker else {
                skip(&mut skipped, texts::REMOVED_SPEAKER.get().to_owned());
                continue;
            };
            let Some(command) = send_before(&mut speaker.state, change, catalog, now) else {
                skip(&mut skipped, speaker.state.display_name());
                continue;
            };
            speaker.handle.send(command);
            self.history.history.mark_taken_back(id, index);
            if let Some(write) = speaker.state.newest_write() {
                self.history.awaited.push(AwaitedTakeBack {
                    speaker: speaker.state.key.clone(),
                    write,
                    entry: id,
                    change: index,
                });
            }
        }
        // Ctrl+Z and the EQ window's undo work with the history closed; it opens to tell this.
        if !skipped.is_empty() {
            self.history.open = true;
        }
        self.history.notice = (!skipped.is_empty()).then_some(TakeBackNotice { skipped });
        Some(id)
    }

    fn changed_since(&self, entry: &Entry, catalog: &Catalog) -> bool {
        entry
            .changes
            .iter()
            .filter(|change| is_open(change))
            .any(|change| {
                let step = paths::catalog_entry(catalog, &change.path).and_then(|entry| entry.step);
                changed_since(change, self.latest(change), step)
            })
    }
}

/// Each speaker once, however many of the entry's changes it left out.
fn skip(skipped: &mut Vec<String>, name: String) {
    if !skipped.contains(&name) {
        skipped.push(name);
    }
}

/// The value before through the write policy; a level as a level, so the fader holds it.
fn send_before(
    speaker: &mut SpeakerState,
    change: &Change,
    catalog: &Catalog,
    now: f64,
) -> Option<Command> {
    if speaker.status != Status::Connected {
        return None;
    }
    let before = change.before.clone()?;
    speaker.request(Intent::restoring(&change.path, before), catalog, now)
}

fn clock_now() -> ClockTime {
    let now = super::local_now();
    ClockTime {
        hour: now.hour(),
        minute: now.minute(),
        second: now.second(),
    }
}

fn clock_text(clock: ClockTime) -> String {
    texts::clock_time(clock.hour, clock.minute, clock.second)
}
