//! When the speaker worker reads which path: meters on their own tick, the fast and slow paths
//! one at a time in turn, so a long round of them never delays the meters.

use std::time::{Duration, Instant};

use crate::catalog::{Entry, Poll};
use crate::speaker::Timing;
use crate::ssc::wire::SscPath;

#[derive(Debug, PartialEq)]
pub enum Due {
    Meters(Vec<SscPath>),
    Path(SscPath),
    Nothing,
}

struct Slot {
    path: SscPath,
    interval: Duration,
    due: Instant,
}

pub struct Schedule {
    meters: Vec<SscPath>,
    meter_interval: Duration,
    next_meter: Instant,
    slots: Vec<Slot>,
    cursor: usize,
}

impl Schedule {
    /// `Once` entries are left out: they are read when connecting only.
    pub fn new(entries: &[&Entry], timing: &Timing, now: Instant) -> Schedule {
        let mut meters = Vec::new();
        let mut slots = Vec::new();
        for entry in entries {
            let interval = match entry.poll {
                Poll::Meter => {
                    meters.push(entry.path.clone());
                    continue;
                }
                Poll::Once => continue,
                Poll::Fast => timing.fast,
                Poll::Slow => timing.slow,
            };
            slots.push(Slot {
                path: entry.path.clone(),
                interval,
                due: now + interval,
            });
        }
        Schedule {
            meters,
            meter_interval: timing.meter,
            next_meter: now + timing.meter,
            slots,
            cursor: 0,
        }
    }

    /// How long until something is due; with nothing left to poll, one meter interval.
    pub fn time_until_due(&self, now: Instant) -> Duration {
        let meter_due = (!self.meters.is_empty()).then_some(self.next_meter);
        let earliest = self
            .slots
            .iter()
            .map(|slot| slot.due)
            .chain(meter_due)
            .min();
        earliest.map_or(self.meter_interval, |due| {
            due.saturating_duration_since(now)
        })
    }

    /// The due item, rescheduled from `now`. Meters go first; of the due paths only the next
    /// one in turn, so every due path gets its read before any path gets a second one.
    pub fn take_due(&mut self, now: Instant) -> Due {
        let meters_due = !self.meters.is_empty() && self.next_meter <= now;
        if meters_due {
            self.next_meter = now + self.meter_interval;
            return Due::Meters(self.meters.clone());
        }
        let Some(index) = self.next_due_slot(now) else {
            return Due::Nothing;
        };
        self.cursor = (index + 1) % self.slots.len();
        let slot = &mut self.slots[index];
        slot.due = now + slot.interval;
        Due::Path(slot.path.clone())
    }

    /// Stops polling `path`, e.g. because the speaker does not have it.
    pub fn forget(&mut self, path: &SscPath) {
        self.meters.retain(|meter| meter != path);
        self.slots.retain(|slot| &slot.path != path);
        if self.cursor >= self.slots.len() {
            self.cursor = 0;
        }
    }

    fn next_due_slot(&self, now: Instant) -> Option<usize> {
        let count = self.slots.len();
        (0..count)
            .map(|offset| (self.cursor + offset) % count)
            .find(|&index| self.slots[index].due <= now)
    }
}
