//! The device panel: every setting of one speaker, opened from its strip and floating beside
//! it. It closes on Escape, its close button, or a click outside it.

use eframe::egui::{Button, Context, Id, Key, Label, Pos2, RichText, Ui, Window};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::speaker::Command;

use super::details;
use crate::display::firmware_version;
use crate::state::SpeakerState;
use crate::texts;

const PANEL_ID: &str = "device_panel";
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
}

/// `opened_now` is true in the frame of the click that opened the panel, which must not count
/// as a click outside.
pub fn show(
    context: &Context,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    anchor: Pos2,
    opened_now: bool,
) -> PanelResponse {
    let mut close = false;
    let mut remove = false;
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
    let outside = position
        .is_some_and(|position| context.layer_id_at(position) != Some(window.response.layer_id));
    let clicked_outside = pressed && outside;
    PanelResponse {
        commands: window.inner.unwrap_or_default(),
        close: close || escape || (clicked_outside && !opened_now),
        remove,
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
