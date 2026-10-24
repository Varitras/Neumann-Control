//! Everything the settings keep by a speaker's identity follows it when the identity changes
//! (its serial became known) and goes when the speaker is removed: its group and place in the
//! order, its strip colour, its own layouts and where its windows were.

use std::collections::BTreeMap;

use neumann_control_core::settings::Settings;

use super::window_places::{place_key, WindowKind};
use crate::groups::{forget_member, rename_member};

/// `old` became `new`; true when anything changed. Where `new` already has a value, it stays.
pub(super) fn rename_member_settings(settings: &mut Settings, old: &str, new: &str) -> bool {
    let mut changed = rename_member(&mut settings.groups, &mut settings.order, old, new);
    changed |= rename_key(&mut settings.strip_colors, old, new);
    let desk = &mut settings.desk;
    changed |= rename_key(&mut desk.speaker_strip_layouts, old, new);
    changed |= rename_key(&mut desk.speaker_compact_layouts, old, new);
    for kind in WindowKind::ALL {
        let (old, new) = (place_key(kind, old), place_key(kind, new));
        changed |= rename_key(&mut settings.windows, &old, &new);
    }
    changed
}

/// The speaker known as any of `ids` was removed; true when anything was forgotten.
pub(super) fn forget_member_settings(settings: &mut Settings, ids: &[String]) -> bool {
    let mut forgotten = forget_member(&mut settings.groups, &mut settings.order, ids);
    for id in ids {
        forgotten |= settings.strip_colors.remove(id).is_some();
        let desk = &mut settings.desk;
        forgotten |= desk.speaker_strip_layouts.remove(id).is_some();
        forgotten |= desk.speaker_compact_layouts.remove(id).is_some();
        for kind in WindowKind::ALL {
            forgotten |= settings.windows.remove(&place_key(kind, id)).is_some();
        }
    }
    forgotten
}

fn rename_key<T>(map: &mut BTreeMap<String, T>, old: &str, new: &str) -> bool {
    let Some(value) = map.remove(old) else {
        return false;
    };
    map.entry(new.to_owned()).or_insert(value);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::settings::{ColorChoice, StoredStripLayout, WindowGeometry};

    const OLD: &str = "address:192.0.2.10";
    const NEW: &str = "serial:SIM0001234";

    /// A speaker with a colour, an own strip and compact layout and a saved EQ window.
    fn known_as(member: &str) -> Settings {
        let mut settings = Settings::default();
        settings.order.push(member.to_owned());
        settings
            .strip_colors
            .insert(member.to_owned(), ColorChoice::Off);
        settings
            .desk
            .speaker_strip_layouts
            .insert(member.to_owned(), StoredStripLayout::default());
        settings
            .desk
            .speaker_compact_layouts
            .insert(member.to_owned(), Vec::new());
        let geometry = WindowGeometry {
            x: 1.0,
            y: 2.0,
            width: 300.0,
            height: 200.0,
            maximized: false,
        };
        settings
            .windows
            .insert(place_key(WindowKind::Eq, member), geometry);
        settings
    }

    fn keeps_anything_of(settings: &Settings, member: &str) -> bool {
        settings.strip_colors.contains_key(member)
            || settings.desk.speaker_strip_layouts.contains_key(member)
            || settings.desk.speaker_compact_layouts.contains_key(member)
            || WindowKind::ALL
                .iter()
                .any(|kind| settings.windows.contains_key(&place_key(*kind, member)))
    }

    #[test]
    fn a_removed_speaker_leaves_no_colour_layout_or_window_place() {
        let mut settings = known_as(NEW);
        assert!(forget_member_settings(&mut settings, &[NEW.to_owned()]));
        assert!(!keeps_anything_of(&settings, NEW));
    }

    #[test]
    fn a_speaker_whose_serial_became_known_keeps_its_colour_layouts_and_windows() {
        let mut settings = known_as(OLD);
        assert!(rename_member_settings(&mut settings, OLD, NEW));
        assert!(!keeps_anything_of(&settings, OLD));
        assert_eq!(settings.strip_colors.get(NEW), Some(&ColorChoice::Off));
        assert!(settings.desk.speaker_strip_layouts.contains_key(NEW));
        assert!(settings.desk.speaker_compact_layouts.contains_key(NEW));
        assert!(settings
            .windows
            .contains_key(&place_key(WindowKind::Eq, NEW)));
    }

    /// Known under both identities, the newer one's colour wins over the older one's.
    #[test]
    fn a_rename_onto_a_known_identity_keeps_its_colour() {
        let mut settings = known_as(OLD);
        settings
            .strip_colors
            .insert(NEW.to_owned(), ColorChoice::Own("#112233".to_owned()));
        rename_member_settings(&mut settings, OLD, NEW);
        assert_eq!(
            settings.strip_colors.get(NEW),
            Some(&ColorChoice::Own("#112233".to_owned()))
        );
    }
}
