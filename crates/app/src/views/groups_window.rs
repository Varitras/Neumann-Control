//! The groups window: every group in a list, the chosen one's settings beside it, adding and
//! deleting. A window of its own, opened from the toolbar, a group strip or a member's name; it
//! closes on Escape or the window's close button.

use eframe::egui::{
    self, Context, Id, ScrollArea, Ui, Vec2, ViewportBuilder, ViewportClass, ViewportId,
};

use super::group_settings::{self, GroupEdit};
use super::{escape_closes, placed, Opening};
use crate::texts;

const WINDOW_ID: &str = "groups_window";
pub(crate) const GROUPS_WINDOW_SIZE: [f32; 2] = [640.0, 520.0];
const GROUPS_WINDOW_MIN_SIZE: [f32; 2] = [420.0, 300.0];
const LIST_WIDTH: f32 = 160.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupsAction {
    None,
    Close,
    Select(usize),
    Add,
    /// The chosen group's settings changed.
    Changed,
    /// Asks before deleting the chosen group.
    Delete,
    ConfirmDelete,
    CancelDelete,
}

/// The groups by name, which one is chosen, and its settings while one is.
pub struct GroupsView<'a> {
    pub names: &'a [String],
    pub selected: Option<usize>,
    /// The delete is being confirmed. A group goes with its members' places and links.
    pub deleting: bool,
    pub edit: Option<GroupEdit<'a>>,
}

pub fn viewport() -> ViewportId {
    ViewportId::from_hash_of(Id::unique(WINDOW_ID))
}

pub fn show(context: &Context, opening: Opening, view: GroupsView) -> GroupsAction {
    let builder = ViewportBuilder::default()
        .with_title(texts::GROUPS.get())
        .with_min_inner_size(GROUPS_WINDOW_MIN_SIZE);
    let builder = placed(builder, opening, Vec2::from(GROUPS_WINDOW_SIZE));
    let mut view = view;
    context.show_viewport_immediate(viewport(), builder, |ui, class| {
        super::note_repaint_causes(ui);
        let mut scrolled = |ui: &mut Ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| contents(ui, &mut view))
                .inner
        };
        let action = match class {
            ViewportClass::EmbeddedWindow => scrolled(ui),
            _ => egui::CentralPanel::default().show(ui, scrolled).inner,
        };
        let escape = escape_closes(ui, Id::unique(WINDOW_ID));
        let close_requested = ui.input(|input| input.viewport().close_requested());
        if escape || close_requested {
            return GroupsAction::Close;
        }
        action
    })
}

fn contents(ui: &mut Ui, view: &mut GroupsView) -> GroupsAction {
    let mut action = GroupsAction::None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(LIST_WIDTH);
            action = list(ui, view.names, view.selected);
        });
        ui.separator();
        ui.vertical(|ui| {
            let chosen = chosen(ui, view);
            if chosen != GroupsAction::None {
                action = chosen;
            }
        });
    });
    action
}

/// The groups, the chosen one marked, and the button that adds one.
fn list(ui: &mut Ui, names: &[String], selected: Option<usize>) -> GroupsAction {
    let mut action = GroupsAction::None;
    if names.is_empty() {
        ui.weak(texts::NO_GROUPS);
    }
    for (index, name) in names.iter().enumerate() {
        if ui.selectable_label(selected == Some(index), name).clicked() {
            action = GroupsAction::Select(index);
        }
    }
    ui.add_space(ui.spacing().item_spacing.y);
    if ui.button(texts::ADD_GROUP).clicked() {
        action = GroupsAction::Add;
    }
    action
}

/// The chosen group's settings, and deleting it.
fn chosen(ui: &mut Ui, view: &mut GroupsView) -> GroupsAction {
    let (Some(edit), Some(index)) = (view.edit.as_mut(), view.selected) else {
        return GroupsAction::None;
    };
    let edit = GroupEdit {
        settings: &mut *edit.settings,
        palette: edit.palette,
        members: &mut *edit.members,
    };
    let mut action = GroupsAction::None;
    if group_settings::edit(ui, edit) {
        action = GroupsAction::Changed;
    }
    ui.separator();
    if !view.deleting {
        if ui.button(texts::DELETE_GROUP).clicked() {
            action = GroupsAction::Delete;
        }
        return action;
    }
    let name = view.names.get(index).map_or("", String::as_str);
    ui.label(texts::delete_question(name.trim()));
    ui.horizontal(|ui| {
        if ui.button(texts::DELETE_GROUP).clicked() {
            action = GroupsAction::ConfirmDelete;
        }
        if ui.button(texts::CANCEL).clicked() {
            action = GroupsAction::CancelDelete;
        }
    });
    action
}
