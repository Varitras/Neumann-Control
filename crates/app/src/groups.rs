//! Groups on the desk: who belongs where, the order of the strips, and the relative moves of a
//! group or master fader. No drawing here.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use neumann_control_core::settings::{GroupSettings, Linkable};
use neumann_control_core::ssc::wire::SscPath;

use crate::paths;
use crate::state::SpeakerKey;

const SERIAL_PREFIX: &str = "serial:";
const ADDRESS_PREFIX: &str = "address:";

/// How a speaker is named in `settings.json`: by serial once known, else by address.
pub fn member_id(key: &SpeakerKey) -> String {
    match key {
        SpeakerKey::Serial(serial) => format!("{SERIAL_PREFIX}{serial}"),
        SpeakerKey::Address(address) => format!("{ADDRESS_PREFIX}{address}"),
    }
}

/// The SSC path a linked setting writes.
pub fn linked_path(linkable: Linkable) -> SscPath {
    let text = match linkable {
        Linkable::Level => paths::OUTPUT_LEVEL,
        Linkable::Mute => paths::MUTE,
        Linkable::Standby => paths::STANDBY_ENABLED,
        Linkable::Input => paths::INPUT_INTERFACE,
    };
    paths::path(text)
}

pub const LINKABLE: [Linkable; 4] = [
    Linkable::Level,
    Linkable::Mute,
    Linkable::Standby,
    Linkable::Input,
];

/// The linked setting a path belongs to, if any.
pub fn linkable_of(path: &SscPath) -> Option<Linkable> {
    LINKABLE
        .into_iter()
        .find(|linkable| linked_path(*linkable) == *path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskItem {
    /// Index into the speakers as the app holds them.
    Speaker(usize),
    /// Index into the groups; drawn after its members.
    GroupStrip(usize),
}

/// The strips from left to right: ungrouped speakers first, then each group's members followed
/// by its group strip. Within each part the saved order decides; speakers it does not know yet
/// follow in the app's order.
pub fn layout(order: &[String], groups: &[GroupSettings], speakers: &[String]) -> Vec<DeskItem> {
    let rank: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(position, id)| (id.as_str(), position))
        .collect();
    let mut sorted: Vec<usize> = (0..speakers.len()).collect();
    sorted.sort_by_key(|index| {
        (
            rank.get(speakers[*index].as_str())
                .copied()
                .unwrap_or(usize::MAX),
            *index,
        )
    });
    let group_of = |index: usize| {
        groups
            .iter()
            .position(|group| group.members.contains(&speakers[index]))
    };
    let mut items: Vec<DeskItem> = sorted
        .iter()
        .copied()
        .filter(|index| group_of(*index).is_none())
        .map(DeskItem::Speaker)
        .collect();
    for (group, _) in groups.iter().enumerate() {
        let members = sorted
            .iter()
            .copied()
            .filter(|index| group_of(*index) == Some(group));
        items.extend(members.map(DeskItem::Speaker));
        items.push(DeskItem::GroupStrip(group));
    }
    items
}

/// Puts `member` into `group` (or into none), out of any other group.
pub fn set_membership(groups: &mut [GroupSettings], member: &str, group: Option<usize>) {
    for existing in groups.iter_mut() {
        existing.members.retain(|id| id != member);
    }
    if let Some(target) = group.and_then(|index| groups.get_mut(index)) {
        target.members.push(member.to_owned());
    }
}

/// Moves `member` in front of `before` in the saved order; speakers the order does not know
/// yet are added first in `speakers` order, so the result covers everyone.
pub fn move_in_order(order: &mut Vec<String>, speakers: &[String], member: &str, before: &str) {
    for id in speakers {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    order.retain(|id| id != member);
    let at = order
        .iter()
        .position(|id| id == before)
        .unwrap_or(order.len());
    order.insert(at, member.to_owned());
}

/// The level a group or master fader shows: its loudest member.
pub fn reference_level(levels: &[f64]) -> Option<f64> {
    levels.iter().copied().reduce(f64::max)
}

/// Every member moved by `delta` dB within its own range; members that would not change are
/// left out, so nothing is written for them.
pub fn shifted_levels<K: Clone>(
    members: &[(K, f64, RangeInclusive<f64>)],
    delta: f64,
) -> Vec<(K, f64)> {
    members
        .iter()
        .filter_map(|(key, level, range)| {
            let shifted = (level + delta).clamp(*range.start(), *range.end());
            (shifted != *level).then(|| (key.clone(), shifted))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, members: &[&str]) -> GroupSettings {
        GroupSettings {
            name: name.to_owned(),
            color: 0,
            members: members.iter().map(|id| (*id).to_owned()).collect(),
            linked: Vec::new(),
        }
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn layout_puts_ungrouped_first_then_each_group_with_its_strip() {
        let speakers = ids(&["a", "b", "c", "d"]);
        let groups = [group("Front", &["c", "a"]), group("Sub", &["d"])];
        let order = ids(&["c", "b", "a"]);
        let items = layout(&order, &groups, &speakers);
        use DeskItem::*;
        assert_eq!(
            items,
            vec![
                Speaker(1),
                Speaker(2),
                Speaker(0),
                GroupStrip(0),
                Speaker(3),
                GroupStrip(1)
            ]
        );
    }

    #[test]
    fn a_speaker_is_in_one_group_at_most() {
        let mut groups = [group("Front", &["a"]), group("Sub", &[])];
        set_membership(&mut groups, "a", Some(1));
        assert!(groups[0].members.is_empty());
        assert_eq!(groups[1].members, ids(&["a"]));
        set_membership(&mut groups, "a", None);
        assert!(groups.iter().all(|group| group.members.is_empty()));
    }

    #[test]
    fn moving_puts_the_speaker_before_the_target() {
        let mut order = ids(&["a", "b"]);
        move_in_order(&mut order, &ids(&["a", "b", "c"]), "c", "a");
        assert_eq!(order, ids(&["c", "a", "b"]));
        move_in_order(&mut order, &ids(&["a", "b", "c"]), "c", "b");
        assert_eq!(order, ids(&["a", "c", "b"]));
    }

    #[test]
    fn group_fader_follows_the_loudest_and_keeps_differences() {
        assert_eq!(reference_level(&[90.0, 96.0]), Some(96.0));
        assert_eq!(reference_level(&[]), None);
        let members = [("a", 90.0, 0.0..=120.0), ("b", 96.0, 0.0..=120.0)];
        assert_eq!(
            shifted_levels(&members, -6.0),
            vec![("a", 84.0), ("b", 90.0)]
        );
    }

    #[test]
    fn a_member_at_its_limit_stays_there() {
        let members = [("a", 118.0, 0.0..=120.0), ("b", 120.0, 0.0..=120.0)];
        assert_eq!(shifted_levels(&members, 5.0), vec![("a", 120.0)]);
    }

    #[test]
    fn linked_settings_map_to_their_paths_and_back() {
        for linkable in LINKABLE {
            assert_eq!(linkable_of(&linked_path(linkable)), Some(linkable));
        }
        assert_eq!(linkable_of(&paths::path(paths::IDENTIFY)), None);
    }
}
