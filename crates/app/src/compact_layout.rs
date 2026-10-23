//! Which buttons a model's compact rows show after their fixed part, and how a change to that
//! list is applied. Like the strip layout, the stored list is resolved against the catalog every
//! time, so a hand-edited or older file never puts a button on a row that the model cannot have.

use neumann_control_core::catalog::{Catalog, Kind};
use neumann_control_core::settings::CompactButton;

use crate::paths;
use crate::strip_layout::fits_a_strip;

#[derive(Clone, Debug, PartialEq)]
pub enum CompactChange {
    Add(CompactButton),
    Remove(CompactButton),
    MoveLeft(CompactButton),
    MoveRight(CompactButton),
    Reset,
}

/// What a row needs while working; the other switches can be added.
pub fn default_buttons(product: &str, catalog: &Catalog) -> Vec<CompactButton> {
    let buttons = [
        CompactButton::Switch(paths::path(paths::MUTE)),
        CompactButton::Eq,
        CompactButton::Details,
    ];
    buttons
        .into_iter()
        .filter(|button| shows(button, product, catalog))
        .collect()
}

/// The stored buttons, or the default ones, without what the model cannot have and without
/// repeats (the first one stays).
pub fn resolve(
    stored: Option<&[CompactButton]>,
    product: &str,
    catalog: &Catalog,
) -> Vec<CompactButton> {
    let buttons = stored.map_or_else(|| default_buttons(product, catalog), <[_]>::to_vec);
    let mut resolved: Vec<CompactButton> = Vec::new();
    for button in buttons {
        if shows(&button, product, catalog) && !resolved.contains(&button) {
            resolved.push(button);
        }
    }
    resolved
}

/// What could still go on the row: the model's switches in catalog order, then EQ and Details.
pub fn addable(buttons: &[CompactButton], product: &str, catalog: &Catalog) -> Vec<CompactButton> {
    let switches = catalog
        .entries_for(product)
        .into_iter()
        .filter(|entry| entry.kind == Kind::Switch && fits_a_strip(entry))
        .map(|entry| CompactButton::Switch(entry.path.clone()));
    switches
        .chain([CompactButton::Eq, CompactButton::Details])
        .filter(|button| shows(button, product, catalog) && !buttons.contains(button))
        .collect()
}

/// The buttons to store after `change`; `None` drops the model's entry, so it shows the default.
pub fn apply(buttons: &[CompactButton], change: &CompactChange) -> Option<Vec<CompactButton>> {
    let mut changed = buttons.to_vec();
    let place = |button: &CompactButton| buttons.iter().position(|other| other == button);
    match change {
        CompactChange::Reset => return None,
        CompactChange::Add(button) if place(button).is_none() => changed.push(button.clone()),
        CompactChange::Add(_) => {}
        CompactChange::Remove(button) => changed.retain(|other| other != button),
        CompactChange::MoveLeft(button) => {
            if let Some(index) = place(button).filter(|index| *index > 0) {
                changed.swap(index, index - 1);
            }
        }
        CompactChange::MoveRight(button) => {
            if let Some(index) = place(button).filter(|index| index + 1 < buttons.len()) {
                changed.swap(index, index + 1);
            }
        }
    }
    Some(changed)
}

/// Whether `button` can be on a compact row of `product`.
fn shows(button: &CompactButton, product: &str, catalog: &Catalog) -> bool {
    match button {
        CompactButton::Details => true,
        CompactButton::Eq => !catalog.eq_containers(product).is_empty(),
        CompactButton::Switch(path) => catalog
            .entries_for(product)
            .into_iter()
            .any(|entry| entry.path == *path && entry.kind == Kind::Switch && fits_a_strip(entry)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: &str = "KH 120 II";
    const SUBWOOFER: &str = "KH 750";

    fn switch(text: &str) -> CompactButton {
        CompactButton::Switch(paths::path(text))
    }

    fn mute_eq_details() -> Vec<CompactButton> {
        vec![
            switch(paths::MUTE),
            CompactButton::Eq,
            CompactButton::Details,
        ]
    }

    #[test]
    fn the_default_is_mute_eq_and_details() {
        assert_eq!(
            default_buttons(MONITOR, Catalog::embedded()),
            mute_eq_details()
        );
        assert_eq!(
            resolve(None, SUBWOOFER, Catalog::embedded()),
            mute_eq_details()
        );
    }

    #[test]
    fn resolve_drops_repeats_and_what_is_no_switch_of_the_model() {
        let stored = [
            switch(paths::MUTE_OUTPUT_A),
            switch(paths::DELAY),
            CompactButton::Details,
            CompactButton::Details,
        ];
        let resolved = resolve(Some(&stored), MONITOR, Catalog::embedded());
        assert_eq!(resolved, vec![CompactButton::Details]);
    }

    #[test]
    fn addable_offers_the_other_switches_but_no_values() {
        let catalog = Catalog::embedded();
        let offered = addable(&mute_eq_details(), SUBWOOFER, catalog);
        for wanted in [paths::IDENTIFY, paths::MUTE_OUTPUT_A, paths::MUTE_OUTPUT_B] {
            assert!(offered.contains(&switch(wanted)), "{wanted}");
        }
        for unwanted in [paths::MUTE, paths::DELAY, paths::TEMPERATURE] {
            assert!(!offered.contains(&switch(unwanted)), "{unwanted}");
        }
        assert!(!offered.contains(&CompactButton::Eq));
        let all_hidden = addable(&[], MONITOR, catalog);
        assert!(all_hidden.ends_with(&[CompactButton::Eq, CompactButton::Details]));
    }

    #[test]
    fn add_remove_and_reset() {
        let identify = switch(paths::IDENTIFY);
        let added = apply(&mute_eq_details(), &CompactChange::Add(identify.clone())).unwrap();
        assert_eq!(added.last(), Some(&identify));
        let again = apply(&added, &CompactChange::Add(identify.clone())).unwrap();
        assert_eq!(again, added);
        let removed = apply(&added, &CompactChange::Remove(identify)).unwrap();
        assert_eq!(removed, mute_eq_details());
        assert_eq!(apply(&removed, &CompactChange::Reset), None);
    }

    #[test]
    fn moving_stops_at_the_ends() {
        let buttons = mute_eq_details();
        let right = apply(&buttons, &CompactChange::MoveRight(switch(paths::MUTE))).unwrap();
        assert_eq!(right[..2], [CompactButton::Eq, switch(paths::MUTE)]);
        let left = apply(&right, &CompactChange::MoveLeft(switch(paths::MUTE))).unwrap();
        assert_eq!(left, buttons);
        let first = apply(&buttons, &CompactChange::MoveLeft(switch(paths::MUTE))).unwrap();
        assert_eq!(first, buttons);
        let last = apply(&buttons, &CompactChange::MoveRight(CompactButton::Details)).unwrap();
        assert_eq!(last, buttons);
    }
}
