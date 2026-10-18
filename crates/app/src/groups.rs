//! Groups on the desk: who belongs where, the order of the strips, and the relative moves of a
//! group or master fader. No drawing here.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{GroupSettings, Linkable};
use neumann_control_core::ssc::wire::SscPath;

use crate::paths;
use crate::state::SpeakerKey;
use crate::texts;

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

/// A linked setting is named as the device panel names it, so a link button and the panel
/// never disagree about which setting it is.
pub fn linkable_label(linkable: Linkable) -> &'static str {
    let path = linked_path(linkable);
    paths::catalog_entry(Catalog::embedded(), &path)
        .map_or("", |entry| entry.label(texts::language()))
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

/// One row of the compact mode: an ungrouped speaker, or a group standing for its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactRow {
    Speaker(usize),
    Group(usize),
}

/// The compact rows in the order of the desk: grouped speakers have no row of their own.
pub fn compact_rows(
    items: &[DeskItem],
    groups: &[GroupSettings],
    speakers: &[String],
) -> Vec<CompactRow> {
    let grouped = |index: usize| {
        groups
            .iter()
            .any(|group| group.members.contains(&speakers[index]))
    };
    items
        .iter()
        .filter_map(|item| match *item {
            DeskItem::Speaker(index) if grouped(index) => None,
            DeskItem::Speaker(index) => Some(CompactRow::Speaker(index)),
            DeskItem::GroupStrip(group) => Some(CompactRow::Group(group)),
        })
        .collect()
}

/// A speaker known by address until it first connects is known by serial from then on; its
/// group and its place in the order follow it. Returns whether anything changed.
pub fn rename_member(
    groups: &mut [GroupSettings],
    order: &mut Vec<String>,
    old: &str,
    new: &str,
) -> bool {
    let mut changed = false;
    for group in groups.iter_mut() {
        changed |= rename_in(&mut group.members, old, new);
    }
    changed | rename_in(order, old, new)
}

/// `old` becomes `new` where it stands; where `new` is already listed, `old` just goes.
fn rename_in(ids: &mut Vec<String>, old: &str, new: &str) -> bool {
    let Some(position) = ids.iter().position(|id| id == old) else {
        return false;
    };
    if ids.iter().any(|id| id == new) {
        ids.remove(position);
    } else {
        ids[position] = new.to_owned();
    }
    true
}

/// A removed speaker leaves its group and its place in the order, under every id it had.
/// Returns whether anything changed.
pub fn forget_member(
    groups: &mut [GroupSettings],
    order: &mut Vec<String>,
    ids: &[String],
) -> bool {
    let known = |id: &String| ids.contains(id);
    let before = order.len()
        + groups
            .iter()
            .map(|group| group.members.len())
            .sum::<usize>();
    order.retain(|id| !known(id));
    for group in groups.iter_mut() {
        group.members.retain(|id| !known(id));
    }
    let after = order.len()
        + groups
            .iter()
            .map(|group| group.members.len())
            .sum::<usize>();
    after != before
}

/// A new group: named `base` with the lowest number no group has, in the palette colour fewest
/// groups have, so no two groups look alike until there are more groups than colours.
pub fn new_group(groups: &[GroupSettings], base: &str, colors: u8) -> GroupSettings {
    let taken = |name: &str| groups.iter().any(|group| group.name == name);
    let name = (1..)
        .map(|number| format!("{base} {number}"))
        .find(|name| !taken(name))
        .expect("an unbounded range always yields a free name");
    let uses = |color: u8| groups.iter().filter(|group| group.color == color).count();
    let color = (0..colors).min_by_key(|color| uses(*color)).unwrap_or(0);
    GroupSettings {
        name,
        color,
        members: Vec::new(),
        linked: Vec::new(),
    }
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

/// Where a dragged strip would land: the number of other strips of its section whose centre
/// is left of the pointer, so past the last one is a slot of its own.
pub fn insertion_slot(centers: &[f32], pointer_x: f32) -> usize {
    centers.iter().filter(|center| **center < pointer_x).count()
}

/// The desk with `dragged` moved to `slot` among `others`, the other speakers of its section in
/// their current order; strips of other sections keep their places.
pub fn with_moved(
    items: &[DeskItem],
    dragged: usize,
    others: &[usize],
    slot: usize,
) -> Vec<DeskItem> {
    let Some(anchor) = others.get(slot).or(others.last()) else {
        return items.to_vec();
    };
    let mut moved: Vec<DeskItem> = items
        .iter()
        .copied()
        .filter(|item| *item != DeskItem::Speaker(dragged))
        .collect();
    let anchor_at = moved
        .iter()
        .position(|item| *item == DeskItem::Speaker(*anchor))
        .unwrap_or(moved.len());
    let at = if slot < others.len() {
        anchor_at
    } else {
        anchor_at + 1
    };
    moved.insert(at, DeskItem::Speaker(dragged));
    moved
}

/// The saved order after a drop: the speakers as `items` shows them, then the saved ids of
/// speakers not on the desk right now, so they keep their places for when they return.
pub fn order_of(items: &[DeskItem], speakers: &[String], saved: &[String]) -> Vec<String> {
    let mut order: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            DeskItem::Speaker(index) => Some(speakers[*index].clone()),
            DeskItem::GroupStrip(_) => None,
        })
        .collect();
    let absent: Vec<String> = saved
        .iter()
        .filter(|id| !order.contains(id))
        .cloned()
        .collect();
    order.extend(absent);
    order
}

/// The level a group or master fader shows: its loudest member.
pub fn reference_level(levels: &[f64]) -> Option<f64> {
    levels.iter().copied().reduce(f64::max)
}

/// What a group or master fader shows and how far it can be pushed.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedLevel {
    /// The loudest member's level.
    pub reference: f64,
    /// Down until the loudest member reaches the bottom of `range`; up until the quietest
    /// reaches its top, so the others still follow while the loudest stays at its limit.
    pub reach: RangeInclusive<f64>,
}

/// None while no member reports a level.
pub fn shared_level(levels: &[f64], range: &RangeInclusive<f64>) -> Option<SharedLevel> {
    let reference = reference_level(levels)?;
    let quietest = levels.iter().copied().reduce(f64::min)?;
    let top = reference + (range.end() - quietest);
    Some(SharedLevel {
        reference,
        reach: *range.start()..=top,
    })
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
    fn compact_rows_show_groups_instead_of_their_members() {
        use DeskItem::*;
        let speakers = ids(&["a", "b", "c"]);
        let groups = [group("Front", &["a", "c"])];
        let items = layout(&[], &groups, &speakers);
        assert_eq!(
            compact_rows(&items, &groups, &speakers),
            vec![CompactRow::Speaker(1), CompactRow::Group(0)]
        );
        assert_eq!(items[0], Speaker(1));
    }

    #[test]
    fn a_renamed_member_keeps_its_group_and_place() {
        let mut groups = [group("Front", &["address:a", "b"])];
        let mut order = ids(&["c", "address:a", "b"]);
        assert!(rename_member(
            &mut groups,
            &mut order,
            "address:a",
            "serial:a"
        ));
        assert_eq!(groups[0].members, ids(&["serial:a", "b"]));
        assert_eq!(order, ids(&["c", "serial:a", "b"]));
        assert!(!rename_member(
            &mut groups,
            &mut order,
            "address:x",
            "serial:x"
        ));
    }

    #[test]
    fn a_rename_onto_a_listed_id_drops_the_old_one() {
        let mut groups = [group("Front", &["serial:a", "address:a"])];
        let mut order = ids(&["serial:a", "address:a"]);
        rename_member(&mut groups, &mut order, "address:a", "serial:a");
        assert_eq!(groups[0].members, ids(&["serial:a"]));
        assert_eq!(order, ids(&["serial:a"]));
    }

    #[test]
    fn a_forgotten_member_leaves_group_and_order_under_every_id() {
        let mut groups = [group("Front", &["serial:a", "b"])];
        let mut order = ids(&["address:a", "b"]);
        let forgotten = ids(&["serial:a", "address:a"]);
        assert!(forget_member(&mut groups, &mut order, &forgotten));
        assert_eq!(groups[0].members, ids(&["b"]));
        assert_eq!(order, ids(&["b"]));
        assert!(!forget_member(&mut groups, &mut order, &forgotten));
    }

    #[test]
    fn a_new_group_takes_a_free_name_and_the_least_used_colour() {
        let mut groups = vec![group("Gruppe 2", &[])];
        groups[0].color = 1;
        let next = new_group(&groups, "Gruppe", 3);
        assert_eq!((next.name.as_str(), next.color), ("Gruppe 1", 0));
        groups.push(next);
        let after = new_group(&groups, "Gruppe", 3);
        assert_eq!((after.name.as_str(), after.color), ("Gruppe 3", 2));
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
    fn the_slot_is_the_number_of_strips_left_of_the_pointer() {
        let centers = [50.0, 150.0, 250.0];
        assert_eq!(insertion_slot(&centers, 10.0), 0);
        assert_eq!(insertion_slot(&centers, 120.0), 1);
        assert_eq!(insertion_slot(&centers, 400.0), 3, "behind the last strip");
    }

    #[test]
    fn a_dragged_strip_moves_within_its_section() {
        use DeskItem::*;
        let items = [Speaker(0), Speaker(1), Speaker(2), GroupStrip(0)];
        // Speaker 0 dragged among the others of its section (1 and 2).
        assert_eq!(
            with_moved(&items, 0, &[1, 2], 2),
            vec![Speaker(1), Speaker(2), Speaker(0), GroupStrip(0)]
        );
        assert_eq!(
            with_moved(&items, 2, &[0, 1], 0),
            vec![Speaker(2), Speaker(0), Speaker(1), GroupStrip(0)]
        );
        assert_eq!(with_moved(&items, 1, &[0, 2], 1), items.to_vec());
    }

    #[test]
    fn the_saved_order_follows_the_desk_and_keeps_absent_speakers() {
        use DeskItem::*;
        let speakers = ids(&["a", "b"]);
        let items = [Speaker(1), GroupStrip(0), Speaker(0)];
        let saved = ids(&["gone", "a", "b"]);
        assert_eq!(
            order_of(&items, &speakers, &saved),
            ids(&["b", "a", "gone"])
        );
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
    fn a_shared_fader_reaches_as_far_as_the_quietest_member_can_follow() {
        let range = 0.0..=120.0;
        let level = shared_level(&[120.0, 90.0], &range).unwrap();
        assert_eq!(level.reference, 120.0);
        assert_eq!(level.reach, 0.0..=150.0);
        assert_eq!(shared_level(&[], &range), None);
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
