//! The device panel: every setting of one speaker, opened from its strip and floating beside
//! it. It closes on Escape, its close button, or a click outside it.

use eframe::egui::{Button, ComboBox, Context, Id, Key, Label, Order, Pos2, RichText, Ui, Window};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::Linkable;
use neumann_control_core::speaker::Command;

use super::details;
use crate::display::firmware_version;
use crate::groups::linkable_label;
use crate::state::SpeakerState;
use crate::texts;

const PANEL_ID: &str = "device_panel";
const GROUP_CHOICE_ID: &str = "group_choice";
const EDITING_ID: &str = "editing";
const PANEL_WIDTH: f32 = 600.0;
/// Room kept free below the panel and for its header and footer.
const PANEL_CHROME_HEIGHT: f32 = 120.0;
const PANEL_MIN_HEIGHT: f32 = 160.0;

pub struct PanelResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    pub close: bool,
    pub remove: bool,
    /// A new group for the speaker (`Some(None)`: no group).
    pub group: Option<Option<usize>>,
    /// A linked setting whose value goes to the other members.
    pub copy_to_group: Option<Linkable>,
}

/// The groups the speaker could be in, which one it is in, and what that group links.
pub struct GroupChoice<'a> {
    pub names: &'a [String],
    pub current: Option<usize>,
    pub linked: &'a [Linkable],
}

/// `opened_now` is true in the frame of the click that opened the panel, which must not count
/// as a click outside.
pub fn show(
    context: &Context,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    anchor: Pos2,
    opened_now: bool,
    groups: &GroupChoice,
) -> PanelResponse {
    let mut close = false;
    let mut remove = false;
    let mut group = None;
    let mut copy_to_group = None;
    // Escape in a field cancels the edit; only Escape with nothing being edited closes. egui
    // drops the focus at the start of the Escape frame, so the last frame's state decides.
    let editing_id = Id::new(PANEL_ID).with(EDITING_ID);
    let editing = context.data(|data| data.get_temp::<bool>(editing_id).unwrap_or(false));
    // As tall as the window allows below the anchor, so rows are not hidden needlessly.
    let room = context.content_rect().bottom() - anchor.y - PANEL_CHROME_HEIGHT;
    let max_height = room.max(PANEL_MIN_HEIGHT);
    let window = Window::new(texts::PANEL_TITLE)
        .id(Id::new(PANEL_ID))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(anchor)
        .min_width(PANEL_WIDTH)
        .max_width(PANEL_WIDTH)
        .max_height(max_height)
        .default_height(max_height)
        .vscroll(true)
        .show(context, |ui| {
            close = header(ui, speaker);
            group = group_choice(ui, groups);
            copy_to_group = copy_buttons(ui, groups);
            let commands = details::show(ui, speaker, catalog);
            ui.separator();
            remove = ui.button(texts::REMOVE).clicked();
            commands
        })
        .expect("the panel window is open");
    let escape = !editing && context.input(|input| input.key_pressed(Key::Escape));
    let editing_now = context.memory(|memory| memory.focused().is_some());
    context.data_mut(|data| data.insert_temp(editing_id, editing_now));
    let pressed = context.input(|input| input.pointer.any_pressed());
    let position = context.input(|input| input.pointer.interact_pos());
    // Popups the panel opens (a combo box's list) lie on layers above it; a click there
    // belongs to the panel.
    let outside = position.is_some_and(|position| {
        let layer = context.layer_id_at(position);
        let on_popup = layer.is_some_and(|layer| layer.order > Order::Middle);
        layer != Some(window.response.layer_id) && !on_popup
    });
    let clicked_outside = pressed && outside;
    PanelResponse {
        commands: window.inner.unwrap_or_default(),
        close: close || escape || (clicked_outside && !opened_now),
        remove,
        group,
        copy_to_group,
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
