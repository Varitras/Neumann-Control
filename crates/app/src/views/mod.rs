pub mod about;
pub mod add_address;
pub mod all_values;
mod color_choice;
pub mod compact;
mod compact_menu;
mod details;
mod details_fields;
mod drag;
pub mod eq_graph;
pub mod eq_table;
pub mod eq_window;
pub mod fader;
pub mod group_dialog;
pub mod group_strip;
pub mod history;
mod knob;
mod layout_menu;
mod meters;
mod number_field;
pub mod panel;
pub mod preset_dialog;
pub mod remove_speaker;
pub mod scan_choice;
pub mod settings_dialog;
pub mod strip;
mod strip_drop;
pub mod system_strip;
pub mod toolbar;
pub mod trends;
pub mod wheel;

use eframe::egui::{self, Id, Key, Ui};

const BUSY_ID: &str = "escape_busy";

/// Whether Escape closes the window `window` (any id of it) in this frame. Escape first ends
/// what is going on in the window: typing in a field, an open list, a drag. egui ends those on
/// that very Escape before the window sees it, so whether one was going on is taken from the
/// frame before. Asked once per frame, after the window's contents.
pub(crate) fn escape_closes(ui: &Ui, window: Id) -> bool {
    let busy_id = window.with(BUSY_ID);
    let was_busy = ui.data(|data| data.get_temp::<bool>(busy_id).unwrap_or(false));
    let context = ui.ctx();
    let busy = ui.memory(|memory| memory.focused().is_some())
        || context.dragged_id().is_some()
        || egui::Popup::is_any_open(context);
    ui.data_mut(|data| data.insert_temp(busy_id, busy));
    !was_busy && !busy && ui.input(|input| input.key_pressed(Key::Escape))
}

/// Where a window of its own opens, and how large; without a position the system places it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlace {
    pub position: Option<eframe::egui::Pos2>,
    pub size: eframe::egui::Vec2,
}
