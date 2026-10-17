//! The top bar: "Suchen", "+ Adresse", the speaker count and a notice when something failed.

use eframe::egui::{self, Button, Ui};

use crate::texts;

const PANEL_ID: &str = "toolbar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    None,
    Scan,
    AddAddress,
}

pub struct ToolbarFacts<'a> {
    pub scanning: bool,
    pub speaker_count: usize,
    pub notice: Option<&'a str>,
}

pub fn show(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
    egui::Panel::top(PANEL_ID)
        .show(ui, |ui| ui.horizontal(|ui| controls(ui, facts)).inner)
        .inner
}

fn controls(ui: &mut Ui, facts: &ToolbarFacts) -> ToolbarAction {
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
    ui.separator();
    ui.label(texts::speaker_count(facts.speaker_count));
    if let Some(notice) = facts.notice {
        ui.separator();
        ui.colored_label(ui.visuals().error_fg_color, notice);
    }
    action
}
