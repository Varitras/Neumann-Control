//! The device panel: every setting of one speaker, in a window of its own that can be moved
//! anywhere, opened from its strip. It closes on Escape, its close button or the window's.

use eframe::egui::{
    self, Button, ComboBox, Context, Id, Label, Rect, RichText, ScrollArea, Ui, Vec2,
    ViewportBuilder, ViewportClass, ViewportId,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::Linkable;
use neumann_control_core::speaker::Command;

use super::details;
use super::escape_closes;
use super::trends::{self, TemperatureTrend, Trends};
use super::wheel;
use super::WindowPlace;
use crate::display::{conversion_of, firmware_version};
use crate::eq::has_eq;
use crate::groups::linkable_label;
use crate::paths;
use crate::state::SpeakerState;
use crate::strip_layout::{LayoutChange, StripLayout};
use crate::texts;

const PANEL_ID: &str = "device_panel";
const GROUP_CHOICE_ID: &str = "group_choice";
const PANEL_WINDOW_SIZE: [f32; 2] = [600.0, 760.0];
const PANEL_WINDOW_MIN_SIZE: [f32; 2] = [420.0, 300.0];
/// From the main window's top left, so the panel is seen to belong to it.
const PANEL_OFFSET: Vec2 = Vec2::new(40.0, 40.0);
/// Kept free below the panel for the taskbar.
const SCREEN_MARGIN: f32 = 60.0;

/// Where a panel first opens, when it has no place of its own yet: at the main window's top
/// left (`main`, screen points), only as tall as the screen (`monitor`) leaves below that: a
/// fixed height let the panel's foot reach below the screen. Without them the system places the
/// window. Fixed when it opens, so it does not follow the main window afterwards.
pub fn first_place(main: Option<Rect>, monitor: Option<Vec2>) -> WindowPlace {
    let full = Vec2::from(PANEL_WINDOW_SIZE);
    let position = main.map(|main| main.min + PANEL_OFFSET);
    let room = match (position, monitor) {
        // The monitor's top is taken as 0: true for the main screen and for screens beside it.
        (Some(position), Some(monitor)) => monitor.y - position.y - SCREEN_MARGIN,
        (None, Some(monitor)) => monitor.y - SCREEN_MARGIN,
        (_, None) => full.y,
    };
    let height = room.clamp(PANEL_WINDOW_MIN_SIZE[1], full.y);
    WindowPlace {
        position,
        size: Vec2::new(full.x, height),
    }
}

pub struct PanelResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    pub close: bool,
    pub remove: bool,
    /// A new group for the speaker (`Some(None)`: no group).
    pub group: Option<Option<usize>>,
    /// A linked setting whose value goes to the other members.
    pub copy_to_group: Option<Linkable>,
    pub open_eq: bool,
    pub save_preset: bool,
    pub load_preset: bool,
    /// A row put on the model's strips or taken off them.
    pub layout: Option<LayoutChange>,
}

/// The groups the speaker could be in, which one it is in, and what that group links.
pub struct GroupChoice<'a> {
    pub names: &'a [String],
    pub current: Option<usize>,
    pub linked: &'a [Linkable],
}

/// The panel in its own window (`viewport`), one per speaker, opened at `placement`.
pub fn show(
    context: &Context,
    viewport: ViewportId,
    placement: WindowPlace,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    groups: &GroupChoice,
    layout: Option<&StripLayout>,
) -> PanelResponse {
    let mut builder = ViewportBuilder::default()
        .with_title(texts::panel_window_title(&speaker.display_name()))
        .with_inner_size(placement.size)
        .with_min_inner_size(PANEL_WINDOW_MIN_SIZE);
    if let Some(position) = placement.position {
        builder = builder.with_position(position);
    }
    context.show_viewport_immediate(viewport, builder, |ui, class| {
        wheel::track_burst(ui.ctx());
        // Several panels can be open; their widgets must not share state.
        let mut response = ui
            .push_id(viewport, |ui| match class {
                ViewportClass::EmbeddedWindow => contents(ui, speaker, catalog, groups, layout),
                _ => {
                    egui::CentralPanel::default()
                        .show(ui, |ui| contents(ui, speaker, catalog, groups, layout))
                        .inner
                }
            })
            .inner;
        let escape = escape_closes(ui, Id::unique(PANEL_ID).with(viewport));
        let close_requested = ui.input(|input| input.viewport().close_requested());
        response.close |= escape || close_requested;
        response
    })
}

/// Everything the panel shows, scrolling when the window is shorter.
fn contents(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    groups: &GroupChoice,
    layout: Option<&StripLayout>,
) -> PanelResponse {
    let has_eq = has_eq(catalog, speaker);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let close = header(ui, speaker);
            let group = group_choice(ui, groups);
            let copy_to_group = copy_buttons(ui, groups);
            let details = details::show(ui, speaker, catalog, layout);
            ui.separator();
            // Without a temperature only the warnings would be left, which the LEDs show.
            let trends = trends_of(ui, speaker, catalog);
            if trends.temperature.is_some() {
                trends::show(ui, &trends);
                ui.separator();
            }
            let (save_preset, load_preset) = ui
                .horizontal(|ui| {
                    let save = ui.button(texts::SAVE_PRESET).clicked();
                    let load = ui.button(texts::LOAD_PRESET).clicked();
                    (save, load)
                })
                .inner;
            let open_eq = has_eq && ui.button(texts::EQ).clicked();
            let remove = ui.button(texts::REMOVE).clicked();
            PanelResponse {
                commands: details.commands,
                layout: details.layout,
                close,
                remove,
                group,
                copy_to_group,
                open_eq,
                save_preset,
                load_preset,
            }
        })
        .inner
}

/// The speaker's trends; the temperature only where the catalog lists it for the model.
fn trends_of<'a>(ui: &Ui, speaker: &'a SpeakerState, catalog: &'a Catalog) -> Trends<'a> {
    let path = paths::path(paths::TEMPERATURE);
    let temperature = catalog
        .entries_for(speaker.product())
        .into_iter()
        .find(|entry| entry.path == path)
        .and_then(|entry| {
            Some(TemperatureTrend {
                trend: &speaker.temperature,
                conversion: conversion_of(entry)?,
                label: entry.label(texts::language()),
            })
        });
    Trends {
        now: ui.input(|input| input.time),
        temperature,
        warnings: &speaker.warnings,
    }
}

/// Name, model, masked serial and firmware; true when the close button was clicked.
fn header(ui: &mut Ui, speaker: &SpeakerState) -> bool {
    let closed = ui
        .horizontal(|ui| {
            ui.add(Label::new(RichText::new(speaker.display_name()).strong()).truncate());
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| ui.add(Button::new(texts::CLOSE).small()).clicked(),
            )
            .inner
        })
        .inner;
    let model = speaker.model().unwrap_or_default().to_owned();
    if let Some(identity) = &speaker.identity {
        let line = texts::status_line(
            &firmware_version(&identity.version),
            &mask_serial(&identity.serial),
        );
        ui.add(Label::new(RichText::new(format!("{model} · {line}")).small().weak()).wrap());
    }
    closed
}

/// "Gruppe: keine / <name>"; returns a different choice.
fn group_choice(ui: &mut Ui, groups: &GroupChoice) -> Option<Option<usize>> {
    let shown = groups
        .current
        .and_then(|index| groups.names.get(index))
        .map_or(texts::NO_GROUP.get(), String::as_str);
    let mut chosen = None;
    ui.horizontal(|ui| {
        let label = ui.label(texts::GROUP);
        ComboBox::from_id_salt(GROUP_CHOICE_ID)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(groups.current.is_none(), texts::NO_GROUP)
                    .clicked()
                {
                    chosen = Some(None);
                }
                for (index, name) in groups.names.iter().enumerate() {
                    let selected = groups.current == Some(index);
                    if ui.selectable_label(selected, name).clicked() {
                        chosen = Some(Some(index));
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    chosen.filter(|choice| *choice != groups.current)
}

/// One button per setting the speaker's group links.
fn copy_buttons(ui: &mut Ui, groups: &GroupChoice) -> Option<Linkable> {
    groups.current?;
    let mut copy = None;
    ui.horizontal_wrapped(|ui| {
        for linkable in groups.linked {
            let text = texts::copy_to_group(linkable_label(*linkable));
            if ui.button(text).clicked() {
                copy = Some(*linkable);
            }
        }
    });
    copy
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    const SCREEN: Vec2 = Vec2::new(1920.0, 1080.0);

    fn main_at(x: f32, y: f32) -> Option<Rect> {
        Some(Rect::from_min_size(pos2(x, y), vec2(1200.0, 700.0)))
    }

    #[test]
    fn the_panel_opens_at_the_main_windows_top_left_in_full_height() {
        let placement = first_place(main_at(100.0, 50.0), Some(SCREEN));
        assert_eq!(placement.position, Some(pos2(100.0, 50.0) + PANEL_OFFSET));
        assert_eq!(placement.size, Vec2::from(PANEL_WINDOW_SIZE));
    }

    /// The panel's foot used to reach below the screen.
    #[test]
    fn a_low_main_window_gives_a_shorter_panel_that_ends_on_the_screen() {
        let placement = first_place(main_at(100.0, 600.0), Some(SCREEN));
        let top = placement.position.unwrap().y;
        assert!(top + placement.size.y <= SCREEN.y - SCREEN_MARGIN + 0.001);
        assert_eq!(placement.size.x, PANEL_WINDOW_SIZE[0]);
    }

    #[test]
    fn a_tiny_screen_still_gets_the_least_panel() {
        let placement = first_place(main_at(0.0, 0.0), Some(vec2(800.0, 200.0)));
        assert_eq!(placement.size.y, PANEL_WINDOW_MIN_SIZE[1]);
    }

    #[test]
    fn without_window_or_screen_the_system_places_it() {
        let placement = first_place(None, None);
        assert_eq!(placement.position, None);
        assert_eq!(placement.size, Vec2::from(PANEL_WINDOW_SIZE));
    }
}
