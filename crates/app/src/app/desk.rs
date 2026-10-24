//! The desk: strips in the saved order with their groups, group strips and the master strip,
//! and what their gestures mean for each speaker: relative moves of a group or the master, and
//! a change on a group member sent on to the other members when the group links that setting.

use std::collections::HashMap;

use eframe::egui::{
    Align, Context, DragAndDrop, Id, Layout, Rect, ScrollArea, Ui, Vec2, ViewportCommand,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{GroupSettings, Linkable};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::{NeumannApp, DESK_SCROLL_ID};
use crate::compact_layout::{self, CompactChange};
use crate::fader::output_level_range;
use crate::groups::{
    group_tint, insertion_slot, layout, linkable_of, linked_path, member_id, new_group, order_of,
    shared_level, shifted_levels, speaker_tint, with_moved, DeskItem,
};
use crate::layout_owner::{store, stored, LayoutMaps, LayoutOwner};
use crate::paths;
use crate::state::{SpeakerKey, SpeakerState, Status};
use crate::strip_layout::{self, LayoutChange};
use crate::texts;
use crate::ui_kit::theme::{group_color_count, tint_color};
use crate::views::group_strip::{self, GroupStripResponse, MasterResponse};
use crate::views::strip::{self, StripView};
use crate::views::system_strip::{self, SystemStrip, SystemStripResponse};
use crate::writes::{can_write, Intent};

const STRIP_CENTERS_ID: &str = "strip_centers";
/// The dragged strip stays where it would land, faded so the strips around it read as the
/// result of the drop.
const DRAGGED_OPACITY: f32 = 0.4;

/// The speaker shown with identity `member`, as the owner of the layout it shows.
pub(super) fn layout_owner_of(speaker: &SpeakerState, member: &str) -> LayoutOwner {
    LayoutOwner {
        product: speaker.product().to_owned(),
        member: member.to_owned(),
    }
}

/// What a frame's gestures asked for; applied once the desk is drawn.
pub(super) enum DeskAction {
    Commands(usize, Vec<Command>),
    Group(usize, GroupStripResponse),
    Master(MasterResponse),
    OpenPanel(SpeakerKey),
    OpenEq(SpeakerKey),
    System(SystemStripResponse),
    /// A change of a speaker's strip layout (its model's, or with own layouts its own).
    Layout(LayoutOwner, LayoutChange),
    /// Like `Layout` for the compact buttons.
    CompactLayout(LayoutOwner, CompactChange),
}

/// The group being renamed or deleted, and the name typed so far.
pub(super) struct GroupDraft {
    pub index: usize,
    pub name: String,
    /// While the delete is being confirmed.
    pub deleting: bool,
}

impl NeumannApp {
    /// While the first window is fitted: as large as the desk's content with the margin it has
    /// on the left and top also on the right and bottom.
    fn fit_window(&mut self, context: &Context, desk: Rect, content: Vec2) {
        let Some(fit) = &mut self.window_fit else {
            return;
        };
        let margin = desk.min.to_vec2();
        let wanted = margin + content + Vec2::splat(margin.x);
        let (current, screen) = context.input(|input| {
            let viewport = input.viewport();
            let current = viewport.inner_rect.map(|inner| inner.size());
            (current, viewport.monitor_size)
        });
        let current = current.unwrap_or_else(|| context.content_rect().size());
        if let Some(size) = fit.next(current, wanted, screen) {
            context.send_viewport_cmd(ViewportCommand::InnerSize(size));
        }
    }

    pub(super) fn show_desk(&mut self, ui: &mut Ui) {
        if self.speakers.is_empty() {
            ui.weak(texts::NO_SPEAKERS);
            return;
        }
        let catalog = Catalog::embedded();
        let desk = self.settings.desk.clone();
        let editing = self.editing;
        let ids = self.member_ids();
        let items = layout(&self.settings.order, &self.settings.groups, &ids);
        let dragged = DragAndDrop::payload::<String>(ui.ctx())
            .and_then(|member| ids.iter().position(|id| *id == *member));
        let items = match dragged {
            Some(index) => previewed(ui.ctx(), items, index, &ids, &self.settings.groups),
            None => items,
        };
        let mut centers = HashMap::new();
        let range = output_level_range();
        let now = ui.input(|input| input.time);
        let system = self.read_system_volume(now);
        let mut actions = Vec::new();
        let scroll = ScrollArea::both()
            .id_salt(DESK_SCROLL_ID)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    for item in &items {
                        match *item {
                            DeskItem::Speaker(index) => {
                                let tint = speaker_tint(
                                    &self.settings.groups,
                                    &self.settings.strip_colors,
                                    &ids[index],
                                );
                                let color = tint_color(&tint);
                                let speaker = &mut self.speakers[index];
                                let key = speaker.state.key.clone();
                                let response = ui
                                    .push_id(&key, |ui| {
                                        if dragged == Some(index) {
                                            ui.multiply_opacity(DRAGGED_OPACITY);
                                        }
                                        let member = &ids[index];
                                        let owner = layout_owner_of(&speaker.state, member);
                                        let stored = stored(
                                            &desk.strip_layouts,
                                            &desk.speaker_strip_layouts,
                                            desk.layouts_per_speaker,
                                            &owner,
                                        );
                                        let layout =
                                            strip_layout::resolve(stored, &owner.product, catalog);
                                        strip::show(
                                            ui,
                                            &mut speaker.state,
                                            catalog,
                                            &desk,
                                            StripView {
                                                layout: &layout,
                                                editing,
                                                layout_key: owner.key(desk.layouts_per_speaker),
                                            },
                                            color,
                                            member,
                                        )
                                    })
                                    .inner;
                                centers.insert(ids[index].clone(), response.rect.center().x);
                                actions.push(DeskAction::Commands(index, response.commands));
                                if response.open_eq {
                                    actions.push(DeskAction::OpenEq(key.clone()));
                                }
                                if let Some(change) = response.layout {
                                    let owner = layout_owner_of(&speaker.state, &ids[index]);
                                    actions.push(DeskAction::Layout(owner, change));
                                }
                                if response.open_panel {
                                    actions.push(DeskAction::OpenPanel(key));
                                }
                            }
                            DeskItem::GroupStrip(group) => {
                                let settings = &self.settings.groups[group];
                                let members = members_of(settings, &ids);
                                let states = self.states(&members);
                                let level = shared_level(&levels(&states), &range);
                                let muted = all_muted(&states);
                                let color = tint_color(&group_tint(settings));
                                let response = ui
                                    .push_id((texts::GROUP, group), |ui| {
                                        let range = range.clone();
                                        let level = level.as_ref();
                                        group_strip::group(ui, settings, color, level, muted, range)
                                    })
                                    .inner;
                                actions.push(DeskAction::Group(group, response));
                            }
                        }
                    }
                    if desk.show_master {
                        let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                        let states = self.states(&everyone);
                        let level = shared_level(&levels(&states), &range);
                        let muted = all_muted(&states);
                        let response =
                            group_strip::master(ui, level.as_ref(), muted, range.clone());
                        actions.push(DeskAction::Master(response));
                    }
                    if let Some(view) = &system {
                        let strip = SystemStrip {
                            name: &view.name,
                            volume: view.volume,
                            mute: view.mute,
                            meters: &self.system_meters,
                            channels: &view.channels,
                            floor: desk.meter_floor.db(),
                        };
                        actions.push(DeskAction::System(system_strip::show(ui, &strip)));
                    }
                });
            });
        self.fit_window(ui.ctx(), scroll.inner_rect, scroll.content_size);
        ui.ctx()
            .data_mut(|data| data.insert_temp(Id::unique(STRIP_CENTERS_ID), centers));
        let dropped = dragged.is_some() && ui.input(|input| input.pointer.any_released());
        if dropped {
            let order = order_of(&items, &ids, &self.settings.order);
            if order != self.settings.order {
                self.settings.order = order;
                self.save_settings();
            }
        }
        let now = ui.input(|input| input.time);
        for action in actions {
            self.apply_desk_action(action, catalog, &ids, now);
        }
    }

    pub(super) fn apply_desk_action(
        &mut self,
        action: DeskAction,
        catalog: &Catalog,
        ids: &[String],
        now: f64,
    ) {
        match action {
            DeskAction::Commands(index, commands) => {
                for command in commands {
                    self.send_with_links(index, command, catalog, ids, now);
                }
            }
            DeskAction::Group(group, response) => {
                let members = members_of(&self.settings.groups[group], ids);
                self.apply_shared(&members, response.level, response.mute, catalog, now);
                if let Some(linkable) = response.toggled_link {
                    toggle_link(&mut self.settings.groups[group], linkable);
                    self.save_settings();
                }
                if response.open_settings || response.delete {
                    let name = self.settings.groups[group].name.clone();
                    self.group_draft = Some(GroupDraft {
                        index: group,
                        name,
                        deleting: response.delete,
                    });
                }
            }
            DeskAction::Master(response) => {
                let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                self.apply_shared(&everyone, response.level, response.mute, catalog, now);
            }
            DeskAction::OpenPanel(key) => self.open_panel(key),
            DeskAction::OpenEq(key) => self.open_eq(key),
            DeskAction::System(response) => self.apply_system_volume(response),
            DeskAction::Layout(owner, change) => self.change_layout(&owner, &change, catalog),
            DeskAction::CompactLayout(owner, change) => {
                self.change_compact_layout(&owner, &change, catalog);
            }
        }
    }

    /// Applies `change` to the layout `owner` shows and saves; a reset drops the entry.
    pub(super) fn change_layout(
        &mut self,
        owner: &LayoutOwner,
        change: &LayoutChange,
        catalog: &Catalog,
    ) {
        let desk = &mut self.settings.desk;
        let current = stored(
            &desk.strip_layouts,
            &desk.speaker_strip_layouts,
            desk.layouts_per_speaker,
            owner,
        );
        let changed = strip_layout::changed(current, &owner.product, catalog, change);
        let maps = LayoutMaps {
            by_model: &mut desk.strip_layouts,
            by_speaker: &mut desk.speaker_strip_layouts,
            per_speaker: desk.layouts_per_speaker,
        };
        store(maps, owner, changed);
        self.save_settings();
    }

    /// Like `change_layout` for the compact buttons.
    fn change_compact_layout(
        &mut self,
        owner: &LayoutOwner,
        change: &CompactChange,
        catalog: &Catalog,
    ) {
        let desk = &mut self.settings.desk;
        let current = stored(
            &desk.compact_layouts,
            &desk.speaker_compact_layouts,
            desk.layouts_per_speaker,
            owner,
        );
        let changed =
            compact_layout::changed(current.map(Vec::as_slice), &owner.product, catalog, change);
        let maps = LayoutMaps {
            by_model: &mut desk.compact_layouts,
            by_speaker: &mut desk.speaker_compact_layouts,
            per_speaker: desk.layouts_per_speaker,
        };
        store(maps, owner, changed);
        self.save_settings();
    }

    /// A group or master fader let go after moving by `moved` dB: every member that may write its
    /// level moves by that much from its own current level, within its own range; a mute switch sets mute on all
    /// of them.
    fn apply_shared(
        &mut self,
        members: &[usize],
        moved: Option<f64>,
        mute: Option<bool>,
        catalog: &Catalog,
        now: f64,
    ) {
        if let Some(moved) = moved {
            let range = output_level_range();
            let current: Vec<(usize, f64, _)> = members
                .iter()
                .filter_map(|index| {
                    level_of(&self.speakers[*index].state)
                        .map(|level| (*index, level, range.clone()))
                })
                .collect();
            for (index, shifted) in shifted_levels(&current, moved) {
                self.request_and_send(index, Intent::Level(shifted), catalog, now);
            }
        }
        if let Some(mute) = mute {
            for index in members {
                let path = paths::path(paths::MUTE);
                let intent = Intent::Write {
                    path,
                    value: Value::Bool(mute),
                };
                self.request_and_send(*index, intent, catalog, now);
            }
        }
    }

    /// Sends a speaker's command, and when its group links the setting, the same value to the
    /// other members whose product may write it.
    pub(super) fn send_with_links(
        &mut self,
        index: usize,
        command: Command,
        catalog: &Catalog,
        ids: &[String],
        now: f64,
    ) {
        let partners = self.link_partners(index, &command, ids);
        self.speakers[index].handle.send(command.clone());
        let Command::Set { path, value } = command else {
            return;
        };
        for partner in partners {
            let intent = Intent::Write {
                path: path.clone(),
                value: value.clone(),
            };
            self.request_and_send(partner, intent, catalog, now);
        }
    }

    /// "Für Gruppe übernehmen": the member's value of `linkable` for all others; a write of it
    /// still on its way counts, since that is what the member will hold.
    pub(super) fn copy_to_group(
        &mut self,
        index: usize,
        linkable: Linkable,
        catalog: &Catalog,
        ids: &[String],
        now: f64,
    ) {
        let path = linked_path(linkable);
        let source = &self.speakers[index].state;
        if source.status != Status::Connected {
            return;
        }
        let Some(value) = source.latest(&path).cloned() else {
            return;
        };
        let Some(group) = group_of(&self.settings.groups, &ids[index]) else {
            return;
        };
        for partner in members_of(&self.settings.groups[group], ids) {
            if partner != index {
                let intent = Intent::Write {
                    path: path.clone(),
                    value: value.clone(),
                };
                self.request_and_send(partner, intent, catalog, now);
            }
        }
    }

    fn link_partners(&self, index: usize, command: &Command, ids: &[String]) -> Vec<usize> {
        let Command::Set { path, .. } = command else {
            return Vec::new();
        };
        let Some(linkable) = linkable_of(path) else {
            return Vec::new();
        };
        let Some(group) = group_of(&self.settings.groups, &ids[index]) else {
            return Vec::new();
        };
        let settings = &self.settings.groups[group];
        if !settings.linked.contains(&linkable) {
            return Vec::new();
        }
        members_of(settings, ids)
            .into_iter()
            .filter(|member| *member != index)
            .collect()
    }

    /// A write on behalf of a gesture elsewhere (group, master, link, copy): a member that is
    /// not connected is left out, so its own message ("nicht erreichbar") stays.
    fn request_and_send(&mut self, index: usize, intent: Intent, catalog: &Catalog, now: f64) {
        let speaker = &mut self.speakers[index];
        if speaker.state.status != Status::Connected {
            return;
        }
        if let Some(command) = speaker.state.request(intent, catalog, now) {
            speaker.handle.send(command);
        }
    }

    pub(super) fn member_ids(&self) -> Vec<String> {
        self.speakers
            .iter()
            .map(|speaker| member_id(&speaker.state.key))
            .collect()
    }

    pub(super) fn add_group(&mut self) {
        let group = new_group(
            &self.settings.groups,
            texts::NEW_GROUP_NAME.get(),
            group_color_count(),
        );
        let name = group.name.clone();
        self.settings.groups.push(group);
        self.save_settings();
        let index = self.settings.groups.len() - 1;
        self.group_draft = Some(GroupDraft {
            index,
            name,
            deleting: false,
        });
    }

    pub(super) fn states(&self, indexes: &[usize]) -> Vec<&SpeakerState> {
        indexes
            .iter()
            .map(|index| &self.speakers[*index].state)
            .collect()
    }
}

/// The desk as it would look with `dragged` dropped at the pointer: moved among the other
/// speakers of its section (its group, or the ungrouped ones), placed by where their strips were
/// last frame. Before every one of them was drawn once, the desk stays as it is.
fn previewed(
    context: &Context,
    items: Vec<DeskItem>,
    dragged: usize,
    ids: &[String],
    groups: &[GroupSettings],
) -> Vec<DeskItem> {
    let section = group_of(groups, &ids[dragged]);
    let others: Vec<usize> = items
        .iter()
        .filter_map(|item| match item {
            DeskItem::Speaker(index) if *index != dragged => Some(*index),
            _ => None,
        })
        .filter(|index| group_of(groups, &ids[*index]) == section)
        .collect();
    let centers: HashMap<String, f32> = context
        .data(|data| data.get_temp(Id::unique(STRIP_CENTERS_ID)))
        .unwrap_or_default();
    let known: Option<Vec<f32>> = others
        .iter()
        .map(|index| centers.get(&ids[*index]).copied())
        .collect();
    let (Some(known), Some(pointer)) = (known, context.pointer_interact_pos()) else {
        return items;
    };
    with_moved(&items, dragged, &others, insertion_slot(&known, pointer.x))
}

pub(super) fn group_of(groups: &[GroupSettings], id: &str) -> Option<usize> {
    groups
        .iter()
        .position(|group| group.members.iter().any(|member| member == id))
}

pub(super) fn members_of(group: &GroupSettings, ids: &[String]) -> Vec<usize> {
    ids.iter()
        .enumerate()
        .filter(|(_, id)| group.members.contains(id))
        .map(|(index, _)| index)
        .collect()
}

fn toggle_link(group: &mut GroupSettings, linkable: Linkable) {
    if group.linked.contains(&linkable) {
        group.linked.retain(|linked| *linked != linkable);
    } else {
        group.linked.push(linkable);
    }
}

/// Only a member that may write its level follows a group or master fader, so only such a
/// member counts for the fader's position and reach.
fn level_of(state: &SpeakerState) -> Option<f64> {
    let path = paths::path(paths::OUTPUT_LEVEL);
    if !can_write(state, Catalog::embedded(), &path) {
        return None;
    }
    state.values.get(&path).and_then(Value::as_f64)
}

pub(super) fn levels(states: &[&SpeakerState]) -> Vec<f64> {
    states.iter().filter_map(|state| level_of(state)).collect()
}

/// Whether every member that may write its mute is muted; none when there is no such member. A
/// member that cannot follow the switch would otherwise keep it from ever offering to unmute.
pub(super) fn all_muted(states: &[&SpeakerState]) -> Option<bool> {
    let path = paths::path(paths::MUTE);
    let mutes: Vec<bool> = states
        .iter()
        .filter(|state| can_write(state, Catalog::embedded(), &path))
        .filter_map(|state| state.values.get(&path).and_then(Value::as_bool))
        .collect();
    (!mutes.is_empty()).then(|| mutes.iter().all(|muted| *muted))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddrV6;

    use neumann_control_core::speaker::{Event, Identity};
    use serde_json::json;

    use super::*;

    /// A connected speaker of `product` reporting `level` and `mute`.
    fn member(product: &str, level: f64, mute: bool) -> SpeakerState {
        let address: SocketAddrV6 = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: product.to_owned(),
            version: "1".to_owned(),
            serial: "SIM".to_owned(),
            name: String::new(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        let reported = [
            (paths::OUTPUT_LEVEL, json!(level)),
            (paths::MUTE, json!(mute)),
        ];
        for (path, value) in reported {
            let path = paths::path(path);
            speaker.apply(Event::Value { path, value }, 0.0);
        }
        speaker
    }

    /// A member that may not write a setting cannot follow the group's control for it, so it
    /// neither sets the fader's reach nor keeps the group from showing it unmuted.
    #[test]
    fn a_read_only_member_does_not_shape_the_group_controls() {
        // Muted the writable one; the read-only one is not: the group must offer to unmute.
        let writable = member("KH 120 II", 80.0, true);
        let read_only = member("KH 80", 120.0, false);
        let states = [&writable, &read_only];
        assert_eq!(levels(&states), vec![80.0]);
        assert_eq!(all_muted(&states), Some(true));
    }
}
