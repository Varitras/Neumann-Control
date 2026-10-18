//! The desk: strips in the saved order with their groups, group strips and the master strip,
//! and what their gestures mean for each speaker: relative moves of a group or the master, and
//! a change on a group member sent on to the other members when the group links that setting.

use std::collections::HashMap;

use eframe::egui::{Align, Context, DragAndDrop, Id, Layout, ScrollArea, Ui};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{GroupSettings, Linkable};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::{NeumannApp, OpenPanel, DESK_SCROLL_ID};
use crate::fader::output_level_range;
use crate::groups::{
    insertion_slot, layout, linkable_of, linked_path, member_id, new_group, order_of, shared_level,
    shifted_levels, with_moved, DeskItem,
};
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::{group_color, group_color_count};
use crate::views::group_strip::{self, GroupStripResponse, MasterResponse};
use crate::views::strip;
use crate::writes::Intent;

const STRIP_CENTERS_ID: &str = "strip_centers";
/// The dragged strip stays where it would land, faded so the strips around it read as the
/// result of the drop.
const DRAGGED_OPACITY: f32 = 0.4;

/// What a frame's gestures asked for; applied once the desk is drawn.
pub(super) enum DeskAction {
    Commands(usize, Vec<Command>),
    Group(usize, GroupStripResponse),
    Master(MasterResponse),
    OpenPanel(OpenPanel),
}

/// The group being renamed or deleted, and the name typed so far.
pub(super) struct GroupDraft {
    pub index: usize,
    pub name: String,
}

impl NeumannApp {
    pub(super) fn show_desk(&mut self, ui: &mut Ui) {
        if self.speakers.is_empty() {
            ui.weak(texts::NO_SPEAKERS);
            return;
        }
        let catalog = Catalog::embedded();
        let desk = self.settings.desk;
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
        let mut actions = Vec::new();
        ScrollArea::both()
            .id_salt(DESK_SCROLL_ID)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    for item in &items {
                        match *item {
                            DeskItem::Speaker(index) => {
                                let color = group_of(&self.settings.groups, &ids[index])
                                    .map(|group| group_color(self.settings.groups[group].color));
                                let speaker = &mut self.speakers[index];
                                let key = speaker.state.key.clone();
                                let response = ui
                                    .push_id(&key, |ui| {
                                        if dragged == Some(index) {
                                            ui.multiply_opacity(DRAGGED_OPACITY);
                                        }
                                        let member = &ids[index];
                                        strip::show(
                                            ui,
                                            &mut speaker.state,
                                            catalog,
                                            &desk,
                                            color,
                                            member,
                                        )
                                    })
                                    .inner;
                                centers.insert(ids[index].clone(), response.rect.center().x);
                                actions.push(DeskAction::Commands(index, response.commands));
                                if let Some(anchor) = response.open_panel {
                                    let opened_now = true;
                                    let open = OpenPanel {
                                        key,
                                        anchor,
                                        opened_now,
                                    };
                                    actions.push(DeskAction::OpenPanel(open));
                                }
                            }
                            DeskItem::GroupStrip(group) => {
                                let settings = &self.settings.groups[group];
                                let members = members_of(settings, &ids);
                                let states = self.states(&members);
                                let level = shared_level(&levels(&states), &range);
                                let muted = all_muted(&states);
                                let color = group_color(settings.color);
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
                    let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                    let states = self.states(&everyone);
                    let level = shared_level(&levels(&states), &range);
                    let muted = all_muted(&states);
                    let response = group_strip::master(ui, level.as_ref(), muted, range.clone());
                    actions.push(DeskAction::Master(response));
                });
            });
        ui.ctx()
            .data_mut(|data| data.insert_temp(Id::new(STRIP_CENTERS_ID), centers));
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
                if response.open_settings {
                    let name = self.settings.groups[group].name.clone();
                    self.group_draft = Some(GroupDraft { index: group, name });
                }
            }
            DeskAction::Master(response) => {
                let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                self.apply_shared(&everyone, response.level, response.mute, catalog, now);
            }
            DeskAction::OpenPanel(open) => self.panel = Some(open),
        }
    }

    /// A group or master fader let go after moving by `moved` dB: every connected member moves by
    /// that much from its own current level, within its own range; a mute switch sets mute on all
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

    /// "Für Gruppe übernehmen": the member's current value of `linkable` for all others.
    pub(super) fn copy_to_group(
        &mut self,
        index: usize,
        linkable: Linkable,
        catalog: &Catalog,
        ids: &[String],
        now: f64,
    ) {
        let path = linked_path(linkable);
        let Some(value) = self.speakers[index].state.values.get(&path).cloned() else {
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
        self.group_draft = Some(GroupDraft { index, name });
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
        .data(|data| data.get_temp(Id::new(STRIP_CENTERS_ID)))
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

fn level_of(state: &SpeakerState) -> Option<f64> {
    if state.status != Status::Connected {
        return None;
    }
    state
        .values
        .get(&paths::path(paths::OUTPUT_LEVEL))
        .and_then(Value::as_f64)
}

pub(super) fn levels(states: &[&SpeakerState]) -> Vec<f64> {
    states.iter().filter_map(|state| level_of(state)).collect()
}

/// Whether every connected member is muted; none when no member is connected.
pub(super) fn all_muted(states: &[&SpeakerState]) -> Option<bool> {
    let mutes: Vec<bool> = states
        .iter()
        .filter(|state| state.status == Status::Connected)
        .filter_map(|state| {
            state
                .values
                .get(&paths::path(paths::MUTE))
                .and_then(Value::as_bool)
        })
        .collect();
    (!mutes.is_empty()).then(|| mutes.iter().all(|muted| *muted))
}
