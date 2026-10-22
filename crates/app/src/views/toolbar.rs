//! The top bar: "Suchen", "+ Adresse", "+ Gruppe", the speaker count, a notice when something
//! failed, and at the right the average response time, "Kompakt", "Verlauf" and "Einstellungen". The
//! compact window is narrow, so there only the count, notice and the right side remain; adding
//! speakers and groups is the desk's job.

use std::time::Duration;

use eframe::egui::{
    self, containers::Sides, Button, RichText, Role, TextStyle, TextWrapMode, Ui, WidgetInfo,
    WidgetText,
};

use crate::texts;

const PANEL_ID: &str = "toolbar";
/// A mark needs less room around it than a word.
const EDIT_BUTTON_PADDING: f32 = 2.0;
/// A separator with the item spacing on both sides.
const SEPARATOR_ROOM: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    None,
    Scan,
    AddAddress,
    AddGroup,
    Settings,
    History,
    ToggleCompact,
    ToggleEditing,
}

pub struct ToolbarFacts {
    pub compact: bool,
    /// Whether the strips are being customized.
    pub editing: bool,
    pub scanning: bool,
    pub speaker_count: usize,
    pub notice: Option<texts::Text>,
    /// None while no speaker has measured one or the settings hide it.
    pub average_response_time: Option<Duration>,
}

pub fn show(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    egui::Panel::top(PANEL_ID)
        .show(ui, |ui| controls(ui, facts))
        .inner
}

/// The right side is placed first, so in a narrow window the count and a notice on the left are
/// cut (whole on hover) instead of being drawn under the buttons.
fn controls(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    let (left, right) = Sides::new()
        .shrink_left()
        .wrap_mode(TextWrapMode::Truncate)
        .show(ui, |ui| left_side(ui, facts), |ui| right_side(ui, facts));
    if right == ToolbarAction::None {
        left
    } else {
        right
    }
}

fn left_side(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    let mut action = ToolbarAction::None;
    if !facts.compact {
        action = adding(ui, facts);
    }
    // Cut to "…" the count would still run under the right side's buttons in the narrowest
    // window; there it is left out, the strips show how many there are.
    let count = texts::speaker_count(facts.speaker_count);
    let needed = text_width(ui, &count) + SEPARATOR_ROOM;
    if facts.compact || needed <= ui.available_width() {
        if !facts.compact {
            ui.separator();
        }
        ui.label(count);
    }
    if let Some(notice) = facts.notice {
        ui.separator();
        ui.colored_label(ui.visuals().error_fg_color, notice);
    }
    action
}

fn text_width(ui: &Ui, text: &str) -> f32 {
    WidgetText::from(text)
        .into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Body,
        )
        .size()
        .x
}

fn right_side(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    let mut action = ToolbarAction::None;
    if ui.button(texts::SETTINGS).clicked() {
        action = ToolbarAction::Settings;
    }
    if ui.button(texts::HISTORY).clicked() {
        action = ToolbarAction::History;
    }
    let compact = Button::new(texts::COMPACT).selected(facts.compact);
    if ui.add(compact).on_hover_text(texts::COMPACT_HINT).clicked() {
        action = ToolbarAction::ToggleCompact;
    }
    if !facts.compact && edit_button(ui, facts.editing) {
        action = ToolbarAction::ToggleEditing;
    }
    if let Some(time) = facts.average_response_time {
        let text = texts::average_response_time(time.as_millis());
        ui.label(RichText::new(text).monospace().weak());
    }
    action
}

/// "Suchen", "+ Adresse" and "+ Gruppe".
fn adding(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    let mut action = ToolbarAction::None;
    if ui
        .add_enabled(!facts.scanning, Button::new(texts::SCAN))
        .clicked()
    {
        action = ToolbarAction::Scan;
    }
    if facts.scanning {
        ui.spinner();
    }
    if ui.button(texts::ADD_ADDRESS).clicked() {
        action = ToolbarAction::AddAddress;
    }
    if ui.button(texts::ADD_GROUP).clicked() {
        action = ToolbarAction::AddGroup;
    }
    action
}

/// "Züge anpassen" / "Fertig" as a mark, not a word: the narrowest window has no room for one
/// more labelled button.
fn edit_button(ui: &mut Ui, editing: bool) -> bool {
    let name = match editing {
        true => texts::DONE.get(),
        false => texts::CUSTOMIZE_STRIPS.get(),
    };
    let edit = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding.x = EDIT_BUTTON_PADDING;
            ui.add(Button::new(texts::EDIT_MARK).selected(editing))
        })
        .inner;
    edit.widget_info(|| WidgetInfo::selected(Role::Button, true, editing, name));
    edit.on_hover_text(name).clicked()
}
