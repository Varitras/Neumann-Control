//! The top bar: "Suchen", "+ Adresse", "+ Gruppe", the speaker count, a notice when something
//! failed, and at the right the average response time, the pen (edit the strips), "Kompakt",
//! "Verlauf", "Einstellungen" and "?" with the manual, the wiki and "Über" (one small button:
//! the narrowest window has no room for two). The compact window is narrow, so there only the
//! count, notice and the right side without the pen remain; adding speakers and groups and
//! editing the strips are the desk's job.

use std::time::Duration;

use eframe::egui::{
    self, containers::Sides, vec2, Button, Color32, Painter, Pos2, RichText, Role, Shape, Stroke,
    TextStyle, TextWrapMode, Ui, WidgetInfo, WidgetText,
};

use crate::texts;

const PANEL_ID: &str = "toolbar";
/// The drawn pen on the edit button, in points.
const PEN_LENGTH: f32 = 13.0;
const PEN_WIDTH: f32 = 3.5;
const PEN_TIP_LENGTH: f32 = 3.5;
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
    Manual,
    Wiki,
    About,
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

/// The right side is placed first, so in a narrow window a notice on the left is cut (whole on
/// hover) and the count left out instead of being drawn under the buttons.
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
    let help = ui.menu_button(texts::HELP, |ui| {
        if ui.button(texts::MANUAL).clicked() {
            action = ToolbarAction::Manual;
        }
        if ui.button(texts::WIKI).clicked() {
            action = ToolbarAction::Wiki;
        }
        if ui.button(texts::ABOUT).clicked() {
            action = ToolbarAction::About;
        }
    });
    help.response.on_hover_text(texts::HELP_HINT);
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

/// "Züge anpassen" / "Fertig" as a drawn pen, not a word: the narrowest window has no room for
/// one more labelled button, and the fonts have no pen glyph.
fn edit_button(ui: &mut Ui, editing: bool) -> bool {
    let name = match editing {
        true => texts::DONE.get(),
        false => texts::CUSTOMIZE_STRIPS.get(),
    };
    let side = ui.spacing().interact_size.y;
    let edit = ui.add(Button::new("").min_size(vec2(side, side)).selected(editing));
    let color = ui
        .style()
        .interact_selectable(&edit, editing)
        .fg_stroke
        .color;
    paint_pen(ui.painter(), edit.rect.center(), color);
    edit.widget_info(|| WidgetInfo::selected(Role::Button, true, editing, name));
    edit.on_hover_text(name).clicked()
}

/// A pen from its tip at the lower left to its end at the upper right.
fn paint_pen(painter: &Painter, center: Pos2, color: Color32) {
    let along = vec2(1.0, -1.0).normalized();
    let across = vec2(1.0, 1.0).normalized() * (PEN_WIDTH / 2.0);
    let tip = center - along * (PEN_LENGTH / 2.0);
    let end = center + along * (PEN_LENGTH / 2.0);
    let shaft = tip + along * PEN_TIP_LENGTH;
    let body = vec![shaft + across, end + across, end - across, shaft - across];
    painter.add(Shape::convex_polygon(body, color, Stroke::NONE));
    let point = vec![tip, shaft + across, shaft - across];
    painter.add(Shape::convex_polygon(point, color, Stroke::NONE));
}
