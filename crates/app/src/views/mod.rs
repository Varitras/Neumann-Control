pub mod about;
pub mod add_address;
pub mod all_values;
pub(crate) mod busy;
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

pub(crate) use meters::METER_FRAME;

use eframe::egui::{self, Context, Id, Key, Role, Ui, UiBuilder, ViewportBuilder};

use crate::frame_stats::cause_name;

const BUSY_ID: &str = "escape_busy";
const REPAINT_CAUSES_ID: &str = "repaint_causes";

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

/// Notes what asked for this frame of the window `ui` draws, for the detailed log: egui keeps a
/// window's causes apart from the main window's, which draws it.
pub(crate) fn note_repaint_causes(ui: &Ui) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let causes: Vec<String> = ui.ctx().repaint_causes().iter().map(cause_name).collect();
    if causes.is_empty() {
        return;
    }
    let id = Id::unique(REPAINT_CAUSES_ID);
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<String>>(id)
            .extend(causes)
    });
}

/// What asked for this frame: the main window's causes and those its windows noted.
pub(crate) fn take_repaint_causes(context: &Context) -> Vec<String> {
    let mut causes: Vec<String> = context.repaint_causes().iter().map(cause_name).collect();
    let id = Id::unique(REPAINT_CAUSES_ID);
    let noted = context.data_mut(|data| data.remove_temp::<Vec<String>>(id));
    causes.extend(noted.unwrap_or_default());
    causes
}

/// `contents` as one region a screen reader names `name`: one strip's "Pegel" or "Stumm" is then
/// told from another's.
pub(crate) fn named_region<R>(ui: &mut Ui, name: &str, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let region = UiBuilder::new()
        .accessibility_role(Role::Group)
        .accessibility_label(name);
    ui.scope_builder(region, contents).inner
}

/// Where a window of its own opens, and how large; without a position the system places it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlace {
    pub position: Option<eframe::egui::Pos2>,
    pub size: eframe::egui::Vec2,
    /// Opens maximized; position and size are where it goes when restored.
    pub maximized: bool,
}

/// How a window of its own is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Opening {
    /// Not open yet: it opens at this place.
    At(WindowPlace),
    /// Not open yet, never left anywhere and the main window's place unknown: the system places
    /// it at its default size.
    Default,
    /// Already open: it stays where and as large as the user has it.
    Open,
}

/// `builder` for a window of `default_size` drawn as `opening` says. An open window gets
/// neither position nor size: egui sends every difference from the last frame's builder to the
/// window, which fought the user moving or sizing it.
pub(crate) fn placed(
    builder: ViewportBuilder,
    opening: Opening,
    default_size: eframe::egui::Vec2,
) -> ViewportBuilder {
    match opening {
        Opening::At(place) => {
            let sized = builder
                .with_inner_size(place.size)
                .with_maximized(place.maximized);
            match place.position {
                Some(position) => sized.with_position(position),
                None => sized,
            }
        }
        Opening::Default => builder.with_inner_size(default_size),
        Opening::Open => builder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    const DEFAULT_SIZE: eframe::egui::Vec2 = vec2(400.0, 300.0);

    #[test]
    fn an_open_window_is_neither_moved_nor_sized() {
        let builder = placed(ViewportBuilder::default(), Opening::Open, DEFAULT_SIZE);
        assert_eq!(builder.position, None);
        assert_eq!(builder.inner_size, None);
    }

    #[test]
    fn a_window_opens_where_it_was_left() {
        let place = WindowPlace {
            position: Some(pos2(10.0, 20.0)),
            size: vec2(500.0, 200.0),
            maximized: false,
        };
        let builder = placed(ViewportBuilder::default(), Opening::At(place), DEFAULT_SIZE);
        assert_eq!(builder.position, Some(pos2(10.0, 20.0)));
        assert_eq!(builder.inner_size, Some(vec2(500.0, 200.0)));
        assert_eq!(builder.maximized, Some(false));
    }

    #[test]
    fn a_window_left_maximized_opens_maximized() {
        let place = WindowPlace {
            position: Some(pos2(10.0, 20.0)),
            size: vec2(500.0, 200.0),
            maximized: true,
        };
        let builder = placed(ViewportBuilder::default(), Opening::At(place), DEFAULT_SIZE);
        assert_eq!(builder.maximized, Some(true));
    }

    #[test]
    fn a_window_never_left_opens_at_its_default_size() {
        let builder = placed(ViewportBuilder::default(), Opening::Default, DEFAULT_SIZE);
        assert_eq!(builder.position, None);
        assert_eq!(builder.inner_size, Some(DEFAULT_SIZE));
    }
}
