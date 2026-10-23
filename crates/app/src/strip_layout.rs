//! Which elements a model's channel strips show besides their fixed part, and how a change to
//! that list is applied. The stored list is resolved against the catalog every time, so a
//! hand-edited or older file never puts something on a strip that the model cannot show.

use neumann_control_core::catalog::{Catalog, Entry, Kind};
use neumann_control_core::settings::{Language, StoredStripLayout, StripElement};

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
    Swap {
        old: StripElement,
        new: StripElement,
    },
    Reset,
}

/// What is looked at or switched while working; set-up values (delay, standby time, phase) stay
/// in the panel. Above the fader only what every model has, so the faders of a mixed desk stand
/// level; each model gets the ones it has of the rest.
const DEFAULT_ABOVE: [&str; 3] = [
    paths::INPUT_INTERFACE,
    paths::CURRENT_INPUT,
    paths::STANDBY_COUNTDOWN,
];
const DEFAULT_BUTTONS: [&str; 5] = [
    paths::MUTE,
    paths::MUTE_OUTPUT_A,
    paths::MUTE_OUTPUT_B,
    paths::IDENTIFY,
    paths::STANDBY_ENABLED,
];
/// After the buttons and EQ: the model's own values.
const DEFAULT_MODEL_VALUES: [&str; 2] = [paths::TEMPERATURE, paths::LOGO_BRIGHTNESS];

/// The working values above the fader; the buttons, EQ and the model's own values below.
pub fn default_layout(product: &str, catalog: &Catalog) -> StoredStripLayout {
    let paths_of = |texts: &[&str]| -> Vec<StripElement> {
        texts
            .iter()
            .map(|text| StripElement::Path(paths::path(text)))
            .filter(|element| shows(element, product, catalog))
            .filter(|element| usable_by_default(element, product, catalog))
            .collect()
    };
    let mut below = paths_of(&DEFAULT_BUTTONS);
    if shows(&StripElement::Eq, product, catalog) {
        below.push(StripElement::Eq);
    }
    below.extend(paths_of(&DEFAULT_MODEL_VALUES));
    StoredStripLayout {
        above: paths_of(&DEFAULT_ABOVE),
        below,
    }
}

/// The stored layout, or the default one, without what the model cannot show and without
/// repeats (the first one stays).
pub fn resolve(
    stored: Option<&StoredStripLayout>,
    product: &str,
    catalog: &Catalog,
) -> StripLayout {
    let layout = stored
        .cloned()
        .unwrap_or_else(|| default_layout(product, catalog));
    let mut seen: Vec<StripElement> = Vec::new();
    let mut keep = |elements: Vec<StripElement>| -> Vec<StripElement> {
        elements
            .into_iter()
            .filter(|element| {
                let new = shows(element, product, catalog) && !seen.contains(element);
                if new {
                    seen.push(element.clone());
                }
                new
            })
            .collect()
    };
    let above = keep(layout.above);
    let below = keep(layout.below);
    StripLayout { above, below }
}

/// What could still be put on the strip, in catalog order, the EQ button last.
pub fn addable(layout: &StripLayout, product: &str, catalog: &Catalog) -> Vec<StripElement> {
    let mut addable: Vec<StripElement> = catalog
        .entries_for(product)
        .into_iter()
        .filter(|entry| fits_a_strip(entry))
        .map(|entry| StripElement::Path(entry.path.clone()))
        .filter(|element| !on_strip(layout, element))
        .collect();
    let eq = StripElement::Eq;
    if shows(&eq, product, catalog) && !on_strip(layout, &eq) {
        addable.push(eq);
    }
    addable
}

/// The layout to store after `change`; `None` drops the model's entry, so it shows the default.
pub fn apply(layout: &StripLayout, change: &LayoutChange) -> Option<StoredStripLayout> {
    let mut changed = layout.clone();
    match change {
        LayoutChange::Reset => return None,
        LayoutChange::Add(element, area) if !on_strip(layout, element) => {
            changed.area_mut(*area).push(element.clone());
        }
        LayoutChange::Add(..) => {}
        LayoutChange::Remove(element) => {
            changed.take(element);
        }
        LayoutChange::MoveTo(element, area, index) => {
            if changed.take(element).is_some() {
                let list = changed.area_mut(*area);
                list.insert((*index).min(list.len()), element.clone());
            }
        }
        LayoutChange::MoveUp(element) => changed.move_up(element),
        LayoutChange::MoveDown(element) => changed.move_down(element),
        LayoutChange::Swap { old, new } => {
            changed.take(new);
            if let Some((area, index)) = changed.place(old) {
                changed.area_mut(area)[index] = new.clone();
            }
        }
    }
    Some(StoredStripLayout {
        above: changed.above,
        below: changed.below,
    })
}

pub fn on_strip(layout: &StripLayout, element: &StripElement) -> bool {
    layout.place(element).is_some()
}

impl StripLayout {
    fn area_mut(&mut self, area: Area) -> &mut Vec<StripElement> {
        match area {
            Area::Above => &mut self.above,
            Area::Below => &mut self.below,
        }
    }

    fn place(&self, element: &StripElement) -> Option<(Area, usize)> {
        let in_area = |list: &[StripElement]| list.iter().position(|other| other == element);
        let above = in_area(&self.above).map(|index| (Area::Above, index));
        above.or_else(|| in_area(&self.below).map(|index| (Area::Below, index)))
    }

    fn take(&mut self, element: &StripElement) -> Option<(Area, usize)> {
        let (area, index) = self.place(element)?;
        self.area_mut(area).remove(index);
        Some((area, index))
    }

    /// One place up; the first below the fader goes to the end above it.
    fn move_up(&mut self, element: &StripElement) {
        match self.place(element) {
            Some((Area::Below, 0)) => {
                self.below.remove(0);
                self.above.push(element.clone());
            }
            Some((area, index)) if index > 0 => self.area_mut(area).swap(index, index - 1),
            _ => {}
        }
    }

    /// One place down; the last above the fader goes to the start below it.
    fn move_down(&mut self, element: &StripElement) {
        let Some((area, index)) = self.place(element) else {
            return;
        };
        let last = self.area_mut(area).len() - 1;
        match area {
            Area::Above if index == last => {
                self.above.remove(index);
                self.below.insert(0, element.clone());
            }
            _ if index < last => self.area_mut(area).swap(index, index + 1),
            _ => {}
        }
    }
}

/// Whether `element` can be drawn on a strip of `product`.
fn shows(element: &StripElement, product: &str, catalog: &Catalog) -> bool {
    match element {
        StripElement::Eq => !catalog.eq_containers(product).is_empty(),
        StripElement::Path(path) => catalog
            .entries_for(product)
            .into_iter()
            .any(|entry| entry.path == *path && fits_a_strip(entry)),
    }
}

/// A control a writable model cannot write (the KH 750's auto standby follows its switch)
/// would sit on every strip greyed out. A model the app writes nothing on (not measured, or not
/// yet connected) keeps them: their state still shows, and the strip keeps its shape when the
/// model becomes known.
fn usable_by_default(element: &StripElement, product: &str, catalog: &Catalog) -> bool {
    let StripElement::Path(path) = element else {
        return true;
    };
    let entries = catalog.entries_for(product);
    let writes_anything = entries.iter().any(|entry| entry.is_writable(product));
    entries
        .iter()
        .find(|entry| entry.path == *path)
        .is_some_and(|entry| {
            entry.kind == Kind::Display || entry.is_writable(product) || !writes_anything
        })
}

/// One value a strip can show: no text field, no band list, no list of numbers, no meter.
fn fits_a_strip(entry: &Entry) -> bool {
    let one_value = !entry.bands && entry.element_labels(Language::German).is_empty();
    let kind = matches!(
        entry.kind,
        Kind::Number | Kind::Switch | Kind::Select | Kind::Display
    );
    kind && one_value
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

    /// Delay and standby time above the fader, mute, identify and EQ below.
    fn two_knobs() -> StripLayout {
        let mut below = elements(&[paths::MUTE, paths::IDENTIFY]);
        below.push(StripElement::Eq);
        StripLayout {
            above: elements(&[paths::DELAY, paths::STANDBY_TIME]),
            below,
        }
    }

    fn stored(layout: StoredStripLayout) -> StripLayout {
        StripLayout {
            above: layout.above,
            below: layout.below,
        }
    }

    /// The same on every model, so the faders of a mixed desk stand level.
    const WORKING_ABOVE: [&str; 3] = [
        paths::INPUT_INTERFACE,
        paths::CURRENT_INPUT,
        paths::STANDBY_COUNTDOWN,
    ];

    #[test]
    fn the_monitor_default_shows_what_is_used_while_working() {
        let layout = monitor_default();
        assert_eq!(layout.above, elements(&WORKING_ABOVE));
        let mut below = elements(&[paths::MUTE, paths::IDENTIFY, paths::STANDBY_ENABLED]);
        below.push(StripElement::Eq);
        below.push(element(paths::LOGO_BRIGHTNESS));
        assert_eq!(layout.below, below);
    }

    /// The KH 750 cannot switch auto standby over the network (its switch does), so the default
    /// leaves that button out; set-up values such as the delay stay in the panel.
    #[test]
    fn the_subwoofer_default_adds_its_outputs_and_its_temperature_below() {
        let layout = resolve(None, SUBWOOFER, Catalog::embedded());
        assert_eq!(layout.above, elements(&WORKING_ABOVE));
        let mut below = elements(&[
            paths::MUTE,
            paths::MUTE_OUTPUT_A,
            paths::MUTE_OUTPUT_B,
            paths::IDENTIFY,
        ]);
        below.push(StripElement::Eq);
        below.push(element(paths::TEMPERATURE));
        assert_eq!(layout.below, below);
    }

    #[test]
    fn a_model_without_writes_keeps_its_buttons_by_default() {
        let unknown = resolve(None, "", Catalog::embedded());
        assert!(unknown.below.contains(&element(paths::MUTE)));
        assert!(unknown.below.contains(&element(paths::STANDBY_ENABLED)));
    }

    #[test]
    fn resolve_drops_what_the_model_cannot_show() {
        let stored = StoredStripLayout {
            above: elements(&[
                paths::DEVICE_NAME,
                PHASE_INVERSION,
                paths::DELAY,
                paths::DELAY,
            ]),
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
        let layout = two_knobs();
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
