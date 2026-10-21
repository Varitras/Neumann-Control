//! Which elements a model's channel strips show besides their fixed part, and how a change to
//! that list is applied. The stored list is resolved against the catalog every time, so a
//! hand-edited or older file never puts something on a strip that the model cannot show.

use neumann_control_core::catalog::{Catalog, Entry, Kind};
use neumann_control_core::settings::{StoredStripLayout, StripElement};

use crate::paths;

/// Above or below the fader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Above,
    Below,
}

/// The elements a strip draws, resolved for one model.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StripLayout {
    pub above: Vec<StripElement>,
    pub below: Vec<StripElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutChange {
    Add(StripElement, Area),
    Remove(StripElement),
    /// To `index` in the stored list of `area`, counted after taking the element out.
    MoveTo(StripElement, Area, usize),
    MoveUp(StripElement),
    MoveDown(StripElement),
    Swap { old: StripElement, new: StripElement },
    Reset,
}

const DEFAULT_ABOVE: [&str; 2] = [paths::DELAY, paths::STANDBY_TIME];
const DEFAULT_BELOW: [&str; 2] = [paths::MUTE, paths::IDENTIFY];

/// Today's strip: delay and standby time above the fader, mute, identify and EQ below.
pub fn default_layout(product: &str, catalog: &Catalog) -> StoredStripLayout {
    todo!()
}

pub fn resolve(
    stored: Option<&StoredStripLayout>,
    product: &str,
    catalog: &Catalog,
) -> StripLayout {
    todo!()
}

/// What could still be put on the strip, in catalog order, the EQ button last.
pub fn addable(layout: &StripLayout, product: &str, catalog: &Catalog) -> Vec<StripElement> {
    todo!()
}

/// The layout to store after `change`; `None` drops the model's entry, so it shows the default.
pub fn apply(layout: &StripLayout, change: &LayoutChange) -> Option<StoredStripLayout> {
    todo!()
}

pub fn on_strip(layout: &StripLayout, element: &StripElement) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::ssc::wire::SscPath;

    const MONITOR: &str = "KH 120 II";
    const SUBWOOFER: &str = "KH 750";
    const PHASE_INVERSION: &str = "audio/out/phaseinversion";
    const TEMPERATURE: &str = "device/temperature";

    fn element(text: &str) -> StripElement {
        StripElement::Path(SscPath::parse(text).unwrap())
    }

    fn elements(texts: &[&str]) -> Vec<StripElement> {
        texts.iter().map(|text| element(text)).collect()
    }

    fn monitor_default() -> StripLayout {
        resolve(None, MONITOR, Catalog::embedded())
    }

    fn stored(layout: StoredStripLayout) -> StripLayout {
        StripLayout {
            above: layout.above,
            below: layout.below,
        }
    }

    #[test]
    fn the_default_is_todays_strip() {
        let layout = monitor_default();
        assert_eq!(layout.above, elements(&[paths::DELAY, paths::STANDBY_TIME]));
        let mut below = elements(&[paths::MUTE, paths::IDENTIFY]);
        below.push(StripElement::Eq);
        assert_eq!(layout.below, below);
        let subwoofer = resolve(None, SUBWOOFER, Catalog::embedded());
        assert!(subwoofer.below.contains(&StripElement::Eq));
    }

    #[test]
    fn resolve_drops_what_the_model_cannot_show() {
        let stored = StoredStripLayout {
            above: elements(&[paths::DEVICE_NAME, PHASE_INVERSION, paths::DELAY, paths::DELAY]),
            below: Vec::new(),
        };
        let layout = resolve(Some(&stored), SUBWOOFER, Catalog::embedded());
        assert_eq!(layout.above, elements(&[paths::DELAY]));
    }

    #[test]
    fn addable_lists_what_is_missing_only() {
        let addable = addable(&monitor_default(), MONITOR, Catalog::embedded());
        assert!(addable.contains(&element(PHASE_INVERSION)));
        for missing in [TEMPERATURE, paths::DEVICE_NAME, paths::MUTE] {
            assert!(!addable.contains(&element(missing)), "{missing}");
        }
        assert!(!addable.contains(&StripElement::Eq));
    }

    #[test]
    fn add_remove_and_swap() {
        let layout = monitor_default();
        let phase = element(PHASE_INVERSION);
        let added = stored(apply(&layout, &LayoutChange::Add(phase.clone(), Area::Below)).unwrap());
        assert_eq!(added.below.last(), Some(&phase));
        let swap = LayoutChange::Swap {
            old: element(paths::MUTE),
            new: phase.clone(),
        };
        let swapped = stored(apply(&layout, &swap).unwrap());
        assert_eq!(swapped.below[0], phase);
        assert!(!swapped.below.contains(&element(paths::MUTE)));
        let removed = stored(apply(&swapped, &LayoutChange::Remove(phase.clone())).unwrap());
        assert!(!removed.below.contains(&phase));
    }

    #[test]
    fn moving_crosses_the_fader_at_the_ends() {
        let layout = monitor_default();
        let standby = element(paths::STANDBY_TIME);
        let down = stored(apply(&layout, &LayoutChange::MoveDown(standby.clone())).unwrap());
        assert_eq!(down.above, elements(&[paths::DELAY]));
        assert_eq!(down.below[0], standby);
        let up = stored(apply(&down, &LayoutChange::MoveUp(standby.clone())).unwrap());
        assert_eq!(up, layout);
        let delay = element(paths::DELAY);
        let top = stored(apply(&layout, &LayoutChange::MoveUp(delay)).unwrap());
        assert_eq!(top, layout);
    }

    #[test]
    fn move_to_places_at_the_stored_index() {
        let change = LayoutChange::MoveTo(StripElement::Eq, Area::Above, 0);
        let moved = stored(apply(&monitor_default(), &change).unwrap());
        assert_eq!(moved.above[0], StripElement::Eq);
        assert!(!moved.below.contains(&StripElement::Eq));
    }

    #[test]
    fn operations_use_the_stored_order_when_an_element_is_absent() {
        let layout = StripLayout {
            above: elements(&[paths::DELAY, PHASE_INVERSION, paths::STANDBY_TIME]),
            below: Vec::new(),
        };
        let change = LayoutChange::MoveUp(element(paths::STANDBY_TIME));
        let moved = stored(apply(&layout, &change).unwrap());
        assert_eq!(
            moved.above,
            elements(&[paths::DELAY, paths::STANDBY_TIME, PHASE_INVERSION])
        );
    }

    #[test]
    fn reset_removes_the_entry() {
        assert_eq!(apply(&monitor_default(), &LayoutChange::Reset), None);
    }

    #[test]
    fn no_element_twice_after_any_change() {
        let layout = monitor_default();
        let phase = element(PHASE_INVERSION);
        let mute = element(paths::MUTE);
        let changes = [
            LayoutChange::Add(mute.clone(), Area::Above),
            LayoutChange::Add(phase.clone(), Area::Above),
            LayoutChange::MoveTo(mute.clone(), Area::Above, 1),
            LayoutChange::MoveDown(element(paths::STANDBY_TIME)),
            LayoutChange::Swap {
                old: mute.clone(),
                new: element(paths::IDENTIFY),
            },
            LayoutChange::Swap {
                old: mute,
                new: phase,
            },
        ];
        for change in &changes {
            let result = stored(apply(&layout, change).unwrap());
            let all: Vec<&StripElement> = result.above.iter().chain(&result.below).collect();
            for element in &all {
                let count = all.iter().filter(|other| **other == *element).count();
                assert_eq!(count, 1, "{change:?}: {element:?}");
            }
        }
    }
}
