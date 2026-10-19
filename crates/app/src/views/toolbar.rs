//! The top bar: "Suchen", "+ Adresse", "+ Gruppe", the speaker count, a notice when something
//! failed, and at the right the average response time, "Kompakt", "Verlauf" and "Einstellungen". The
//! compact window is narrow, so there only the count, notice and the right side remain; adding
//! speakers and groups is the desk's job.

use std::time::Duration;

use eframe::egui::{self, containers::Sides, Button, RichText, TextWrapMode, Ui};

use crate::texts;

const PANEL_ID: &str = "toolbar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    None,
    Scan,
    AddAddress,
    AddGroup,
    Settings,
    History,
    ToggleCompact,
}

pub struct ToolbarFacts {
    pub compact: bool,
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
        ui.separator();
    }
    ui.label(texts::speaker_count(facts.speaker_count));
    if let Some(notice) = facts.notice {
        ui.separator();
        ui.colored_label(ui.visuals().error_fg_color, notice);
    }
    action
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
