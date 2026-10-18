//! The desk: strips in the saved order with their groups, group strips and the master strip,
//! and what their gestures mean for each speaker: relative moves of a group or the master, and
//! a change on a group member sent on to the other members when the group links that setting.

use eframe::egui::{Align, Layout, ScrollArea, Ui};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{GroupSettings, Linkable};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::{NeumannApp, OpenPanel, DESK_SCROLL_ID};
use crate::fader::output_level_range;
use crate::groups::{
    layout, linkable_of, linked_path, member_id, reference_level, shifted_levels, DeskItem,
};
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::{group_color, group_color_count};
use crate::views::group_strip::{self, GroupStripResponse, MasterResponse};
use crate::views::strip;
use crate::writes::Intent;

/// What a frame's gestures asked for; applied once the desk is drawn.
enum DeskAction {
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
        let range = output_level_range();
        let unit = level_unit(catalog);
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
                                        strip::show(ui, &mut speaker.state, catalog, &desk, color)
                                    })
                                    .inner;
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
                                let reference = reference_level(&levels(&states));
                                let muted = all_muted(&states);
                                let color = group_color(settings.color);
                                let response = ui
                                    .push_id((texts::GROUP, group), |ui| {
                                        let range = range.clone();
                                        group_strip::group(
                                            ui, settings, color, reference, muted, range, &unit,
                                        )
                                    })
                                    .inner;
                                actions.push(DeskAction::Group(group, response));
                            }
                        }
                    }
                    let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                    let states = self.states(&everyone);
                    let reference = reference_level(&levels(&states));
                    let muted = all_muted(&states);
                    let response = group_strip::master(ui, reference, muted, range.clone(), &unit);
                    actions.push(DeskAction::Master(response));
                });
            });
        let now = ui.input(|input| input.time);
        for action in actions {
            self.apply_desk_action(action, catalog, &ids, now);
        }
    }

    fn apply_desk_action(
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

    /// A group or master fader let go at `level`: every connected member moves by the same
    /// amount within its own range; a mute switch sets mute on all of them.
    fn apply_shared(
        &mut self,
        members: &[usize],
        level: Option<f64>,
        mute: Option<bool>,
        catalog: &Catalog,
        now: f64,
    ) {
        if let Some(level) = level {
            let states = self.states(members);
            if let Some(reference) = reference_level(&levels(&states)) {
                let range = output_level_range();
                let current: Vec<(usize, f64, _)> = members
                    .iter()
                    .filter_map(|index| {
                        level_of(&self.speakers[*index].state)
                            .map(|level| (*index, level, range.clone()))
                    })
                    .collect();
                for (index, shifted) in shifted_levels(&current, level - reference) {
                    self.request_and_send(index, Intent::Level(shifted), catalog, now);
                }
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

    fn request_and_send(&mut self, index: usize, intent: Intent, catalog: &Catalog, now: f64) {
        let speaker = &mut self.speakers[index];
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
        let count = self.settings.groups.len();
        let name = format!("{} {}", texts::NEW_GROUP_NAME, count + 1);
        let color = (count % usize::from(group_color_count())) as u8;
        self.settings.groups.push(GroupSettings {
            name: name.clone(),
            color,
            members: Vec::new(),
            linked: Vec::new(),
        });
        self.save_settings();
        self.group_draft = Some(GroupDraft { index: count, name });
    }

    fn states(&self, indexes: &[usize]) -> Vec<&SpeakerState> {
        indexes
            .iter()
            .map(|index| &self.speakers[*index].state)
            .collect()
    }
}

pub(super) fn group_of(groups: &[GroupSettings], id: &str) -> Option<usize> {
    groups
        .iter()
        .position(|group| group.members.iter().any(|member| member == id))
}

fn members_of(group: &GroupSettings, ids: &[String]) -> Vec<usize> {
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

fn levels(states: &[&SpeakerState]) -> Vec<f64> {
    states.iter().filter_map(|state| level_of(state)).collect()
}

/// Whether every connected member is muted; none when no member is connected.
fn all_muted(states: &[&SpeakerState]) -> Option<bool> {
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

fn level_unit(catalog: &Catalog) -> String {
    paths::catalog_entry(catalog, &paths::path(paths::OUTPUT_LEVEL))
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default()
}
