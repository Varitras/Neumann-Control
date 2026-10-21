//! The session's history on the app side: the writes of each frame become one entry, and an
//! entry is taken back by writing its values before through the normal write policy.

use eframe::egui::Ui;

use super::NeumannApp;
use serde_json::Value;

use crate::eq::container_of;
use crate::history::{changed_since, is_open, Change, ClockTime, Entry, History};
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::views::history::{
    self as history_view, HistoryAction, HistoryRow, QuestionAction, UNDO_SHORTCUT,
};
use crate::writes::Intent;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::speaker::Command;

/// What the last take-back could not write, for the history window.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeBackNotice {
    pub entry: u64,
    pub written: usize,
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
}

/// How a change reads: whose, which setting, and its unit.
struct Described {
    speaker: String,
    setting: String,
    unit: Option<String>,
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
}

impl NeumannApp {
    pub(super) fn open_history(&mut self) {
        self.history.open = true;
        self.history.notice = None;
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
            HistoryAction::Close => self.history.open = false,
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
        let reason = if all_taken_back {
            texts::TAKEN_BACK.get()
        } else {
            texts::BEFORE_UNKNOWN.get()
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
        let unit = described.unit.as_deref();
        let before = change
            .before
            .as_ref()
            .and_then(|before| described.shown(before));
        let before = texts::history_before(before, unit);
        let after = described.shown(&change.after).unwrap_or(&change.after);
        let after = texts::history_value(after, unit);
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
            let unit = described.unit.as_deref();
            let now = described.shown(now).unwrap_or(now);
            let written = described.shown(&change.after).unwrap_or(&change.after);
            lines.push(texts::changed_now(
                &described.speaker,
                &described.setting,
                &texts::history_value(now, unit),
                &texts::history_value(written, unit),
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
        let speaker = self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == change.speaker)
            .map_or_else(
                || texts::REMOVED_SPEAKER.get().to_owned(),
                |speaker| speaker.state.display_name(),
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
        let (setting, band, all_bands) = match (container, given.as_slice()) {
            (Some(container), [band]) => {
                let setting = texts::eq_change_setting(container.label(language), band + 1, &label);
                (setting, Some(*band), false)
            }
            (Some(container), [_, _, ..]) => {
                let setting = texts::eq_all_bands_setting(container.label(language), &label);
                (setting, None, true)
            }
            _ => (label, None, false),
        };
        Described {
            speaker,
            setting,
            unit: entry.and_then(|entry| entry.unit.clone()),
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
                    .map(move |(path, before, after)| Change {
                        speaker: key.clone(),
                        path,
                        before,
                        after,
                        taken_back: false,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.history.history.push(clock_now(), changes, takes_back);
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
        let mut written = 0;
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
                skipped.push(texts::REMOVED_SPEAKER.get().to_owned());
                continue;
            };
            let sent = send_before(&mut speaker.state, change, catalog, now);
            match sent {
                Some(command) => {
                    speaker.handle.send(command);
                    self.history.history.mark_taken_back(id, index);
                    written += 1;
                }
                None => skipped.push(speaker.state.display_name()),
            }
        }
        self.history.notice = (!skipped.is_empty()).then_some(TakeBackNotice {
            entry: id,
            written,
            skipped,
        });
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
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    ClockTime {
        hour: now.hour(),
        minute: now.minute(),
        second: now.second(),
    }
}

fn clock_text(clock: ClockTime) -> String {
    texts::clock_time(clock.hour, clock.minute, clock.second)
}
