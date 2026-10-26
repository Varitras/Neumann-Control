//! The compact mode: one row per ungrouped speaker and per group, the master at the bottom. The
//! rows' gestures mean the same as on the desk and go the same way to the speakers.

use eframe::egui::{Context, ScrollArea, Ui};
use neumann_control_core::catalog::Catalog;

use super::desk::{all_muted, layout_owner_of, levels, members_of, DeskAction};
use super::{NeumannApp, COMPACT_SCROLL_ID};
use crate::compact_layout;
use crate::fader::output_level_range;
use crate::groups::{
    compact_rows, group_tint, layout, reference_level, shared_level, speaker_tint, CompactRow,
};
use crate::layout_owner::stored;
use crate::texts;
use crate::ui_kit::theme::tint_color;
use crate::views::compact::{self, SharedRow};
use crate::window_fit::WindowFit;

impl NeumannApp {
    pub(super) fn show_compact(&mut self, ui: &mut Ui) {
        if self.speakers.is_empty() {
            ui.weak(texts::NO_SPEAKERS);
            return;
        }
        let catalog = Catalog::embedded();
        let desk = self.settings.desk.clone();
        let ids = self.member_ids();
        let items = layout(&self.settings.order, &self.settings.groups, &ids);
        let rows = compact_rows(&items, &self.settings.groups, &ids);
        let floor = desk.meter_floor.db();
        let now = ui.input(|input| input.time);
        let mut actions = Vec::new();
        // auto_shrink off: the scroll area takes all of it.
        let room = ui.available_size();
        // Both ways: below the rows' width the window scrolls instead of hiding mute.
        let scroll = ScrollArea::both()
            .id_salt(COMPACT_SCROLL_ID)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    match *row {
                        CompactRow::Speaker(index) => {
                            let tint = speaker_tint(
                                &self.settings.groups,
                                &self.settings.strip_colors,
                                &ids[index],
                            );
                            let color = tint_color(&tint);
                            let speaker = &mut self.speakers[index];
                            let key = speaker.state.key.clone();
                            let owner = layout_owner_of(&speaker.state, &ids[index]);
                            let stored = stored(
                                &desk.compact_layouts,
                                &desk.speaker_compact_layouts,
                                desk.layouts_per_speaker,
                                &owner,
                            );
                            let buttons = compact_layout::resolve(
                                stored.map(Vec::as_slice),
                                &owner.product,
                                catalog,
                            );
                            let state = &mut speaker.state;
                            let response = ui
                                .push_id(&key, |ui| {
                                    compact::speaker(ui, state, catalog, &desk, &buttons, color)
                                })
                                .inner;
                            actions.push(DeskAction::Commands(index, response.commands));
                            if response.open_eq {
                                actions.push(DeskAction::OpenEq(key.clone()));
                            }
                            if response.open_panel {
                                actions.push(DeskAction::OpenPanel(key));
                            }
                            if let Some(change) = response.layout {
                                actions.push(DeskAction::CompactLayout(owner, change));
                            }
                        }
                        CompactRow::Group(group) => {
                            let settings = &self.settings.groups[group];
                            let members = members_of(settings, &ids);
                            let shown = self.shared_row(&members, floor, now);
                            let color = tint_color(&group_tint(settings));
                            let response = ui
                                .push_id((texts::GROUP, group), |ui| {
                                    compact::group(ui, settings, color, shown)
                                })
                                .inner;
                            actions.push(DeskAction::Group(group, response));
                        }
                    }
                }
                if self.settings.desk.show_master {
                    let everyone: Vec<usize> = (0..self.speakers.len()).collect();
                    let shown = self.shared_row(&everyone, floor, now);
                    actions.push(DeskAction::Master(compact::master(ui, shown)));
                }
            });
        self.fit_window(ui.ctx(), room, scroll.content_size);
        for action in actions {
            self.apply_desk_action(action, catalog, &ids, now);
        }
    }

    fn shared_row(&self, members: &[usize], floor: f64, now: f64) -> SharedRow {
        let states = self.states(members);
        let inputs: Vec<f64> = states
            .iter()
            .filter_map(|state| state.loudest_input(now))
            .collect();
        let range = output_level_range();
        let message = states.iter().find_map(|state| {
            let message = texts::describe_message(state.message.as_ref()?);
            Some(texts::member_message(&state.display_name(), &message))
        });
        SharedRow {
            level: shared_level(&levels(&states), &range),
            muted: all_muted(&states),
            loudest: reference_level(&inputs),
            range,
            floor,
            message,
        }
    }

    /// Desk to compact or back: the window the new mode last had comes back, fitted onto the
    /// monitors there are now, else it is fitted to the content.
    pub(super) fn toggle_compact(&mut self, context: &Context) {
        // Edit mode drags strip elements; compact buttons change by right click instead.
        self.editing = false;
        let left = self.chosen_window();
        let target = self.settings.switch_mode(left);
        log::debug!(
            "mode switch (compact {}): kept {left:?}, now {target:?}",
            self.settings.desk.compact
        );
        self.save_settings();
        // Only a geometry of the new mode may be kept for it; a maximised window reports none
        // and would otherwise leave the old mode's geometry to be saved for the new one.
        self.window = target;
        self.window_fit = target.is_none().then(WindowFit::default);
        // Without a size of its own the fit sizes the window: a first size sent beside it looked
        // like the user's and ended the fit.
        if let Some(geometry) = target {
            self.restore_window(context, geometry);
        }
    }
}
