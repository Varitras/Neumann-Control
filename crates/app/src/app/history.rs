//! The session's history on the app side: the writes of each frame become one entry, and an
//! entry is taken back by writing its values before through the normal write policy.

use eframe::egui::Ui;

use super::NeumannApp;
use serde_json::Value;

use crate::display::{conversion_of, converted};
use crate::eq::{container_of, output_group, several_outputs, take_back_rank, WriteStage};
use crate::history::{
    changed_since, closed_because, is_open, Change, ClockTime, Closed, Entry, History,
};
use crate::paths;
use crate::state::{SpeakerKey, SpeakerState, Status};
use crate::texts;
use crate::views::history::{
    self as history_view, HistoryAction, HistoryRow, QuestionAction, UNDO_SHORTCUT,
};
use crate::writes::{self, Intent};
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

impl HistoryState {
    pub(super) fn rename_speaker(&mut self, before: &SpeakerKey, after: &SpeakerKey) {
        self.history.rename_speaker(before, after);
        for awaited in self
            .awaited
            .iter_mut()
            .filter(|awaited| awaited.speaker == *before)
        {
            awaited.speaker = after.clone();
        }
    }
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
    /// Which bands of an EQ array the write touched; its values are their entries.
    bands: Bands,
}

/// The bands of an EQ write a history line names.
enum Bands {
    /// No EQ array: the value is shown whole.
    Whole,
    One(usize),
    /// Every band (a reset to flat).
    All,
    /// Some bands: one part of a write switching bands off and on, whose other places are
    /// `null`.
    Some(Vec<usize>),
}

impl Described {
    /// The part of `value` the line shows: the band's entry of an EQ array, one entry for
    /// all bands holding the same, the touched bands' entries, else all of it.
    fn shown(&self, value: &Value) -> Option<Value> {
        let entries = value.as_array();
        match &self.bands {
            Bands::One(band) => entries.and_then(|entries| entries.get(*band)).cloned(),
            Bands::All => {
                let first = entries.and_then(|entries| entries.first());
                let alike = entries.is_some_and(|entries| entries.iter().all(|e| Some(e) == first));
                if alike {
                    return first.cloned();
                }
                Some(value.clone())
            }
            Bands::Some(bands) => {
                let entries = entries?;
                let touched = bands.iter().filter_map(|band| entries.get(*band)).cloned();
                Some(Value::Array(touched.collect()))
            }
            Bands::Whole => Some(value.clone()),
        }
    }

    /// `value` as the panel shows it.
    fn text(&self, value: &Value) -> String {
        let Some(shown) = self.shown(value) else {
            return texts::UNKNOWN_VALUE.to_owned();
        };
        match (self.conversion, shown.as_f64()) {
            (Some(conversion), Some(number)) => converted(conversion, number),
            _ => texts::history_value(&shown, self.unit.as_deref()),
        }
    }
}

impl NeumannApp {
    pub(super) fn open_history(&mut self) {
        self.history.open = true;
    }

    /// Ctrl+Z: takes back the newest open entry. Not while a text field has the keyboard (it
    /// has its own undo) or a dialog waits for an answer; a button reached with Tab takes none.
    pub(super) fn undo_shortcut(&mut self, ui: &mut Ui) {
        let typing = ui.ctx().text_edit_focused();
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

    /// A dialog waits for an answer: Ctrl+Z takes nothing back meanwhile, in no window.
    pub(super) fn dialog_open(&self) -> bool {
        self.settings_open
            || self.removal.is_some()
            || self.add_address.is_some()
            || self.scan_choice.is_some()
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
                let setting = one_setting_on_each(&entry.changes)
                    .then(|| self.describe(&entry.changes[0]).setting);
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
        let reason = match closed_because(&entry.changes) {
            Closed::TakenBack => texts::TAKEN_BACK.get(),
            Closed::NotWritten => texts::NOT_WRITTEN.get(),
            Closed::PartlyNotWritten => texts::PARTLY_NOT_WRITTEN.get(),
            Closed::BeforeUnknown => texts::BEFORE_UNKNOWN.get(),
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
        self.changed_now(entry)
            .map(|(change, now)| {
                let described = self.describe(change);
                texts::changed_now(
                    &described.speaker,
                    &described.setting,
                    &described.text(now),
                    &described.text(&change.after),
                )
            })
            .collect()
    }

    /// The open changes of `entry` whose value changed since, with the value now.
    fn changed_now<'a>(
        &'a self,
        entry: &'a Entry,
    ) -> impl Iterator<Item = (&'a Change, &'a serde_json::Value)> {
        let catalog = Catalog::embedded();
        entry
            .changes
            .iter()
            .filter(|change| is_open(change))
            .filter_map(move |change| {
                let latest = self.latest(change);
                let step = paths::catalog_entry(catalog, &change.path).and_then(|entry| entry.step);
                let now = latest.filter(|_| changed_since(change, latest, step))?;
                Some((change, now))
            })
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
        let label = paths::setting_label(catalog, &change.path);
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
        let places = change.after.as_array().map_or(0, Vec::len);
        let (setting, bands) = match (container_name, given.as_slice()) {
            (Some(container), [band]) => {
                let setting = texts::eq_change_setting(&container, band + 1, &label);
                (setting, Bands::One(*band))
            }
            (Some(container), [_, _, ..]) if given.len() == places => {
                let setting = texts::eq_all_bands_setting(&container, &label);
                (setting, Bands::All)
            }
            (Some(container), [_, _, ..]) => {
                let numbers: Vec<usize> = given.iter().map(|band| band + 1).collect();
                let setting = texts::eq_bands_setting(&container, &numbers, &label);
                (setting, Bands::Some(given))
            }
            _ => (label, Bands::Whole),
        };
        Described {
            speaker,
            setting,
            unit: entry.and_then(|entry| entry.unit.clone()),
            conversion: entry.and_then(conversion_of),
            bands,
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

    /// Last in `logic`: files what it wrote (a global shortcut, a dim going back) as its own
    /// entry. `logic` also runs while the window is hidden and `ui` does not, so without this the
    /// answers to those writes would be settled before the writes are filed, and missed.
    pub(super) fn finish_logic_history(&mut self) {
        self.settle_refused_writes();
        let changes = self.written_changes();
        self.history.history.push(clock_now(), changes, None);
    }

    /// Last in `ui`: runs a requested take-back, then files everything the drawing wrote, the
    /// take-back's own writes included, as one entry (`logic` filed its own before).
    pub(super) fn finish_history_frame(&mut self, now: f64) {
        self.settle_refused_writes();
        let mut takes_back = None;
        if let Some((id, confirmed)) = self.history.request.take() {
            takes_back = self.take_back(id, confirmed, now);
        }
        let changes = self.written_changes();
        self.history.history.push(clock_now(), changes, takes_back);
    }

    fn written_changes(&mut self) -> Vec<Change> {
        self.speakers
            .iter_mut()
            .flat_map(|speaker| written_changes(&mut speaker.state))
            .collect()
    }

    /// Closes the changes the speaker refused or never got, and opens again the ones their
    /// take-back was; forgets the take-backs answered.
    fn settle_refused_writes(&mut self) {
        for speaker in &mut self.speakers {
            let refused = settle_speaker(&mut self.history.history, &mut speaker.state);
            let key = &speaker.state.key;
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
        if !confirmed && self.changed_now(&entry).next().is_some() {
            self.history.question = Some(id);
            return None;
        }
        let mut skipped = Vec::new();
        // All or nothing per speaker, as forward: one value the policy refuses keeps the
        // speaker's other changes of the entry from going out alone.
        let refusing: Vec<SpeakerKey> = entry
            .changes
            .iter()
            .filter(|change| is_open(change) && self.refuses_take_back(change, catalog))
            .map(|change| change.speaker.clone())
            .collect();
        // As forward: EQ bands go off first and on last, so no prefix sounds a mixed band.
        let mut order: Vec<(usize, &Change)> = entry.changes.iter().enumerate().collect();
        order.sort_by_key(|(_, change)| {
            change
                .before
                .as_ref()
                .map_or(WriteStage::Between, |before| {
                    take_back_rank(&change.path, before, &change.after)
                })
        });
        for (index, change) in order {
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
            if refusing.contains(&speaker.state.key) {
                skip(&mut skipped, speaker.state.display_name());
                continue;
            }
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

    /// The write policy refuses the change's value before, e.g. one the speaker reported out of
    /// its range; a speaker gone or a value before unknown is left to `take_back` itself.
    fn refuses_take_back(&self, change: &Change, catalog: &Catalog) -> bool {
        let speaker = self
            .speakers
            .iter()
            .find(|speaker| speaker.state.key == change.speaker);
        let (Some(speaker), Some(before)) = (speaker, change.before.as_ref()) else {
            return false;
        };
        writes::check(&speaker.state, catalog, &change.path, before).is_err()
    }
}

/// The writes the speaker was sent since the history last took them, as its changes.
fn written_changes(speaker: &mut SpeakerState) -> Vec<Change> {
    let key = speaker.key.clone();
    speaker
        .written
        .drain(..)
        .map(|written| Change {
            speaker: key.clone(),
            path: written.path,
            before: written.before,
            after: written.after,
            taken_back: false,
            write: Some(written.write),
            refused: false,
        })
        .collect()
}

/// Closes the changes of the writes the speaker refused or never got, and corrects the value
/// before of later writes that took a refused or differently confirmed one; returns the refused
/// numbers.
fn settle_speaker(history: &mut History, speaker: &mut SpeakerState) -> Vec<u64> {
    for predecessor in std::mem::take(&mut speaker.predecessors) {
        let confirmed = predecessor.confirmed.as_ref();
        history.pass_on_before(
            &speaker.key,
            predecessor.write,
            &predecessor.later,
            confirmed,
        );
    }
    let refused = std::mem::take(&mut speaker.refused_writes);
    for write in &refused {
        history.mark_refused(&speaker.key, *write);
    }
    refused
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

/// Whether the changes are one setting on as many speakers, as a group move writes it; the two
/// parts of an EQ write that switches bands off and on are one speaker's.
fn one_setting_on_each(changes: &[Change]) -> bool {
    let Some(first) = changes.first() else {
        return false;
    };
    let shared = changes.iter().all(|change| change.path == first.path);
    let speakers_apart = changes.iter().enumerate().all(|(place, change)| {
        changes[..place]
            .iter()
            .all(|earlier| earlier.speaker != change.speaker)
    });
    shared && speakers_apart
}

#[cfg(test)]
mod tests {
    use neumann_control_core::speaker::{Event, Identity};
    use neumann_control_core::ssc::wire::DeviceError;
    use serde_json::json;

    use super::*;

    const NOON: ClockTime = ClockTime {
        hour: 12,
        minute: 0,
        second: 0,
    };

    /// Band 1 sent twice before the first answer, and the speaker confirms the first at another
    /// value: taking the second back writes the confirmed value, not the one sent.
    #[test]
    fn a_take_back_restores_what_the_speaker_confirmed_before_it() {
        let catalog = Catalog::embedded();
        let gain = paths::path("audio/out/eq2/gain");
        let address = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: "KH 120 II".to_owned(),
            version: "1".to_owned(),
            serial: "SIM0001234".to_owned(),
            name: String::new(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        let reported = Event::Value {
            path: gain.clone(),
            value: json!([0.0, 0.0]),
        };
        speaker.apply(reported, 0.0);
        let mut history = History::default();
        for value in [1.0, 2.0] {
            speaker.record_written(&gain, &json!([value, null]));
            history.push(NOON, written_changes(&mut speaker), None);
        }
        for value in [1.5, 2.0] {
            let confirmed = Event::Written {
                path: gain.clone(),
                value: json!([value, 0.0]),
            };
            speaker.apply(confirmed, 0.0);
        }
        settle_speaker(&mut history, &mut speaker);
        let second = history.newest(0).unwrap().changes[0].clone();
        assert_eq!(second.before, Some(json!([1.5, null])));
        let command = send_before(&mut speaker, &second, catalog, 0.0);
        let expected = Command::Set {
            path: gain,
            value: json!([1.5, null]),
        };
        assert_eq!(command, Some(expected));
    }

    /// Three writes of one band: the first confirmed differently, the second refused. The third
    /// started from the refused value, so it goes back to what the first left.
    #[test]
    fn a_refusal_after_a_differing_confirmation_passes_on_the_confirmed_value() {
        let gain = paths::path("audio/out/eq2/gain");
        let address = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let reported = Event::Value {
            path: gain.clone(),
            value: json!([0.0, 0.0]),
        };
        speaker.apply(reported, 0.0);
        let mut history = History::default();
        for value in [1.0, 2.0, 3.0] {
            speaker.record_written(&gain, &json!([value, null]));
            history.push(NOON, written_changes(&mut speaker), None);
        }
        let confirmed = Event::Written {
            path: gain.clone(),
            value: json!([1.5, 0.0]),
        };
        speaker.apply(confirmed, 0.0);
        let rejected = Event::WriteRejected {
            path: gain.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        speaker.apply(rejected, 0.0);
        settle_speaker(&mut history, &mut speaker);
        let third = &history.newest(0).unwrap().changes[0];
        assert_eq!(third.before, Some(json!([1.5, null])));
    }

    /// Mute on, then off again before the first answer: the speaker refuses the first, by an
    /// error or by a 404, and takes the second. Taking the second back writes what the speaker
    /// held before both, not the value it refused (a 404 once sent `mute true`).
    #[test]
    fn a_take_back_does_not_restore_a_value_the_speaker_refused() {
        let mute = paths::path(paths::MUTE);
        let rejected = Event::WriteRejected {
            path: mute.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        for refusal in [rejected, Event::WriteAbsent(mute.clone())] {
            take_back_after_a_refusal(refusal);
        }
    }

    fn take_back_after_a_refusal(refusal: Event) {
        let catalog = Catalog::embedded();
        let mute = paths::path(paths::MUTE);
        let address = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: "KH 120 II".to_owned(),
            version: "1".to_owned(),
            serial: "SIM0001234".to_owned(),
            name: String::new(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        let reported = Event::Value {
            path: mute.clone(),
            value: json!(false),
        };
        speaker.apply(reported, 0.0);
        let mut history = History::default();
        for value in [true, false] {
            speaker.record_written(&mute, &json!(value));
            history.push(NOON, written_changes(&mut speaker), None);
        }
        let shown = format!("{refusal:?}");
        speaker.apply(refusal, 0.0);
        let written = Event::Written {
            path: mute.clone(),
            value: json!(false),
        };
        speaker.apply(written, 0.0);
        settle_speaker(&mut history, &mut speaker);
        let second = history.newest(0).unwrap().changes[0].clone();
        let command = send_before(&mut speaker, &second, catalog, 0.0);
        let expected = Command::Set {
            path: mute,
            value: json!(false),
        };
        assert_eq!(command, Some(expected), "after {shown}");
    }
}
