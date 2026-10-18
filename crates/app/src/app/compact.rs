//! The compact mode: one row per ungrouped speaker and per group, the master at the bottom. The
//! rows' gestures mean the same as on the desk and go the same way to the speakers.

use eframe::egui::{Context, ScrollArea, Ui, ViewportCommand};
use neumann_control_core::catalog::Catalog;

use super::desk::{all_muted, level_unit, levels, members_of, DeskAction};
use super::{NeumannApp, OpenPanel, COMPACT_SCROLL_ID, DESK_WINDOW_SIZE};
use crate::fader::output_level_range;
use crate::groups::{compact_rows, layout, reference_level, CompactRow};
use crate::texts;
use crate::ui_kit::theme::group_color;
use crate::views::compact::{self, SharedRow};

/// The window size the compact mode starts with before it was ever resized, in points.
const COMPACT_WINDOW_SIZE: [f32; 2] = [640.0, 360.0];

impl NeumannApp {
    pub(super) fn show_compact(&mut self, ui: &mut Ui) {
        if self.speakers.is_empty() {
            ui.weak(texts::NO_SPEAKERS);
            return;
        }
        let catalog = Catalog::embedded();
        let desk = self.settings.desk;
        let ids = self.member_ids();
        let items = layout(&self.settings.order, &self.settings.groups, &ids);
        let rows = compact_rows(&items, &self.settings.groups, &ids);
        let unit = level_unit(catalog);
        let floor = desk.meter_floor.db();
        let now = ui.input(|input| input.time);
        let mut actions = Vec::new();
        ScrollArea::vertical()
            .id_salt(COMPACT_SCROLL_ID)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    match *row {
                        CompactRow::Speaker(index) => {
                            let speaker = &mut self.speakers[index];
                            let key = speaker.state.key.clone();
                            let response = ui
                                .push_id(&key, |ui| {
                                    compact::speaker(ui, &mut speaker.state, catalog, &desk)
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
                        CompactRow::Group(group) => {
                            let settings = &self.settings.groups[group];
                            let members = members_of(settings, &ids);
                            let shown = self.shared_row(&members, &unit, floor, now);
                            let color = group_color(settings.color);
                            let response = ui
                                .push_id((texts::GROUP, group), |ui| {
                                    compact::group(ui, settings, color, shown)
                                })
                                .inner;
                            actions.push(DeskAction::Group(group, response));
                        }
                    }
                }
                let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                let shown = self.shared_row(&everyone, &unit, floor, now);
                actions.push(DeskAction::Master(compact::master(ui, shown)));
            });
        for action in actions {
            self.apply_desk_action(action, catalog, &ids, now);
        }
    }

    fn shared_row<'a>(
        &self,
        members: &[usize],
        unit: &'a str,
        floor: f64,
        now: f64,
    ) -> SharedRow<'a> {
        let states = self.states(members);
        let inputs: Vec<f64> = states
            .iter()
            .filter_map(|state| state.loudest_input(now))
            .collect();
        SharedRow {
            reference: reference_level(&levels(&states)),
            muted: all_muted(&states),
            loudest: reference_level(&inputs),
            range: output_level_range(),
            unit,
            floor,
        }
    }

    /// Desk to compact or back: the window the new mode last had comes back, else its first size.
    pub(super) fn toggle_compact(&mut self, context: &Context) {
        let target = self.settings.switch_mode(self.window);
        self.save_settings();
        let Some(target) = target else {
            let [width, height] = if self.settings.desk.compact {
                COMPACT_WINDOW_SIZE
            } else {
                DESK_WINDOW_SIZE
            };
            context.send_viewport_cmd(ViewportCommand::InnerSize(eframe::egui::vec2(
                width, height,
            )));
            return;
        };
        let points = |physical: f32| physical / context.pixels_per_point();
        let position = eframe::egui::pos2(points(target.x), points(target.y));
        let size = eframe::egui::vec2(points(target.width), points(target.height));
        context.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        context.send_viewport_cmd(ViewportCommand::InnerSize(size));
        self.window = Some(target);
    }
}
