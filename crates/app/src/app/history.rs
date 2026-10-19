//! The session's history on the app side: the writes of each frame become one entry, and an
//! entry is taken back by writing its values before through the normal write policy.

use super::NeumannApp;
use crate::history::{changed_since, is_open, Change, ClockTime, Entry, History};
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
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
}

impl NeumannApp {
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
                let speaker = self
                    .speakers
                    .iter()
                    .find(|speaker| speaker.state.key == change.speaker);
                let reported = speaker.and_then(|speaker| speaker.state.values.get(&change.path));
                let step = paths::catalog_entry(catalog, &change.path).and_then(|entry| entry.step);
                changed_since(change, reported, step)
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
    let level = before
        .as_f64()
        .filter(|_| change.path == paths::path(paths::OUTPUT_LEVEL));
    let intent = match level {
        Some(level) => Intent::Level(level),
        None => Intent::Write {
            path: change.path.clone(),
            value: before,
        },
    };
    speaker.request(intent, catalog, now)
}

fn clock_now() -> ClockTime {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    ClockTime {
        hour: now.hour(),
        minute: now.minute(),
        second: now.second(),
    }
}
