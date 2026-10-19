//! The writes of this session, newest last: one entry per user action, each change of it
//! revertible on its own. Lives only as long as the app runs.

use std::collections::VecDeque;

use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::state::SpeakerKey;

/// A session that runs for days still keeps a bounded list.
pub const HISTORY_LIMIT: usize = 500;

/// The wall clock time of an entry, as the user reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// One write to one speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub speaker: SpeakerKey,
    pub path: SscPath,
    /// `None` when the speaker had not reported the value: such a change cannot be taken back.
    pub before: Option<Value>,
    pub after: Value,
    pub taken_back: bool,
}

/// Everything one user action wrote: a gesture, a group move, a linked write.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub clock: ClockTime,
    pub changes: Vec<Change>,
    /// The entry this one took back.
    pub takes_back: Option<u64>,
}

#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    next_id: u64,
}

impl History {
    /// Adds the changes of one frame as an entry; a frame without writes adds nothing.
    pub fn push(
        &mut self,
        clock: ClockTime,
        changes: Vec<Change>,
        takes_back: Option<u64>,
    ) -> Option<u64> {
        if changes.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push_back(Entry {
            id,
            clock,
            changes,
            takes_back,
        });
        if self.entries.len() > HISTORY_LIMIT {
            self.entries.pop_front();
        }
        Some(id)
    }

    /// Newest first, as the history window lists them.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev()
    }

    pub fn entry(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The newest entry with a change that can still be taken back: what Ctrl+Z takes back.
    pub fn newest_open(&self) -> Option<u64> {
        self.entries()
            .find(|entry| entry.changes.iter().any(is_open))
            .map(|entry| entry.id)
    }

    pub fn mark_taken_back(&mut self, id: u64, change: usize) {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id);
        if let Some(change) = entry.and_then(|entry| entry.changes.get_mut(change)) {
            change.taken_back = true;
        }
    }
}

pub fn is_open(change: &Change) -> bool {
    !change.taken_back && change.before.is_some()
}

/// Whether a reported value still is the written one: numbers within half a catalog step (the
/// speaker may report 84 for a written 84.0, or round to its own grid), anything else exactly.
pub fn same_value(reported: &Value, written: &Value, step: Option<f64>) -> bool {
    let (Some(reported_number), Some(written_number)) = (reported.as_f64(), written.as_f64())
    else {
        return reported == written;
    };
    let tolerance = step.map_or(0.0, |step| step / 2.0);
    (reported_number - written_number).abs() < tolerance || reported_number == written_number
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::paths;

    const NOON: ClockTime = ClockTime {
        hour: 12,
        minute: 0,
        second: 0,
    };

    fn change(before: Option<Value>, after: Value) -> Change {
        Change {
            speaker: SpeakerKey::Serial("SIM0001234".to_owned()),
            path: paths::path(paths::OUTPUT_LEVEL),
            before,
            after,
            taken_back: false,
        }
    }

    #[test]
    fn empty_frames_make_no_entry() {
        let mut history = History::default();
        assert_eq!(history.push(NOON, Vec::new(), None), None);
        assert_eq!(history.entries().count(), 0);
    }

    #[test]
    fn entries_come_newest_first() {
        let mut history = History::default();
        let first = history.push(NOON, vec![change(Some(json!(80)), json!(84))], None);
        let second = history.push(NOON, vec![change(Some(json!(84)), json!(90))], first);
        let ids: Vec<u64> = history.entries().map(|entry| entry.id).collect();
        assert_eq!(ids, vec![second.unwrap(), first.unwrap()]);
        assert_eq!(history.entry(second.unwrap()).unwrap().takes_back, first);
    }

    #[test]
    fn the_oldest_entry_drops_out_at_the_limit() {
        let mut history = History::default();
        let first = history.push(NOON, vec![change(Some(json!(1)), json!(2))], None);
        for _ in 0..HISTORY_LIMIT {
            history.push(NOON, vec![change(Some(json!(1)), json!(2))], None);
        }
        assert_eq!(history.entries().count(), HISTORY_LIMIT);
        assert!(history.entry(first.unwrap()).is_none());
    }

    #[test]
    fn newest_open_skips_taken_back_and_unknown_before() {
        let mut history = History::default();
        let known = history.push(NOON, vec![change(Some(json!(80)), json!(84))], None);
        let unknown = history.push(NOON, vec![change(None, json!(84))], None);
        assert_eq!(history.newest_open(), known);
        assert!(!is_open(
            &history.entry(unknown.unwrap()).unwrap().changes[0]
        ));
        history.mark_taken_back(known.unwrap(), 0);
        assert_eq!(history.newest_open(), None);
    }

    #[test]
    fn numbers_within_half_a_step_are_the_same() {
        assert!(same_value(&json!(84), &json!(84.0), Some(0.5)));
        assert!(same_value(&json!(84.0), &json!(84.2), Some(0.5)));
        assert!(!same_value(&json!(84.0), &json!(84.5), Some(0.5)));
        assert!(!same_value(&json!(84.0), &json!(84.2), None));
        assert!(same_value(&json!(true), &json!(true), None));
        assert!(!same_value(&json!("a"), &json!("b"), Some(1.0)));
    }
}
