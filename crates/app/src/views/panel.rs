//! The device panel: every setting of one speaker, in a window of its own that can be moved
//! anywhere, opened from its strip. It closes on Escape, its close button or the window's.

use eframe::egui::{
    self, Button, ComboBox, Context, Id, Key, Label, RichText, ScrollArea, Ui, ViewportBuilder,
    ViewportClass, ViewportId,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::Linkable;
use neumann_control_core::speaker::Command;

use super::details;
use super::trends::{self, TemperatureTrend, Trends};
use crate::display::firmware_version;
use crate::eq::has_eq;
use crate::groups::linkable_label;
use crate::paths;
use crate::state::SpeakerState;
use crate::strip_layout::{LayoutChange, StripLayout};
use crate::texts;

const PANEL_ID: &str = "device_panel";
const GROUP_CHOICE_ID: &str = "group_choice";
const EDITING_ID: &str = "editing";
const PANEL_WINDOW_SIZE: [f32; 2] = [600.0, 760.0];
const PANEL_WINDOW_MIN_SIZE: [f32; 2] = [420.0, 300.0];

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

/// The panel in its own window (`viewport`), one per speaker.
pub fn show(
    context: &Context,
    viewport: ViewportId,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    groups: &GroupChoice,
    layout: Option<&StripLayout>,
) -> PanelResponse {
    let builder = ViewportBuilder::default()
        .with_title(texts::panel_window_title(&speaker.display_name()))
        .with_inner_size(PANEL_WINDOW_SIZE)
        .with_min_inner_size(PANEL_WINDOW_MIN_SIZE);
    context.show_viewport_immediate(viewport, builder, |ui, class| {
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
        // Escape in a field cancels the edit; only Escape with nothing being edited closes. egui
        // drops the focus at the start of the Escape frame, so the last frame's state decides.
        let editing_id = Id::unique(PANEL_ID).with(viewport).with(EDITING_ID);
        let editing = ui.data(|data| data.get_temp::<bool>(editing_id).unwrap_or(false));
        let escape = !editing && ui.input(|input| input.key_pressed(Key::Escape));
        let editing_now = ui.memory(|memory| memory.focused().is_some());
        ui.data_mut(|data| data.insert_temp(editing_id, editing_now));
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
            trends::show(ui, &trends_of(ui, speaker, catalog));
            ui.separator();
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
                conversion: entry.conversion?,
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
