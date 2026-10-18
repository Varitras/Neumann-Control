//! The strip of a group (name, link buttons, group fader, mute) and the master strip at the end
//! of the desk. Both only report gestures; the app turns them into one write per speaker.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Button, Color32, CornerRadius, RichText, Stroke, Ui, WidgetInfo, WidgetType,
};
use neumann_control_core::settings::{GroupSettings, Linkable};

use super::fader;
use crate::groups::LINKABLE;
use crate::texts;
use crate::ui_kit::theme::Tokens;

const STRIP_WIDTH: f32 = 96.0;
const STRIP_MARGIN: i8 = 6;
const STRIP_RADIUS: u8 = 6;
const BORDER_WIDTH: f32 = 1.0;

#[derive(Default)]
pub struct GroupStripResponse {
    /// The level the group fader was let go at.
    pub level: Option<f64>,
    /// The mute every member should get.
    pub mute: Option<bool>,
    pub toggled_link: Option<Linkable>,
    pub open_settings: bool,
}

/// `reference` is the loudest connected member, `muted` whether all of them are muted.
pub fn group(
    ui: &mut Ui,
    group: &GroupSettings,
    color: Color32,
    reference: Option<f64>,
    muted: Option<bool>,
    range: RangeInclusive<f64>,
    unit: &str,
) -> GroupStripResponse {
    let mut response = GroupStripResponse::default();
    frame(ui, color, |ui| {
        let name = RichText::new(&group.name).strong().color(color);
        response.open_settings = ui.add(Button::new(name).frame(false).truncate()).clicked();
        ui.add(egui::Label::new(
            RichText::new(texts::LINKED).small().weak(),
        ));
        for linkable in LINKABLE {
            let text = texts::linkable_name(linkable);
            let on = group.linked.contains(&linkable);
            let button = ui.add(Button::new(RichText::new(text).small()).selected(on));
            let label = texts::link_label(text.get());
            button.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, &label));
            if button.clicked() {
                response.toggled_link = Some(linkable);
            }
        }
        let label = texts::group_level(&group.name);
        response.level = fader::shared(ui, &label, reference, range, unit);
        let readout = reference.map(texts::shared_readout).unwrap_or_default();
        ui.add(egui::Label::new(RichText::new(readout).monospace()));
        response.mute = mute_button(ui, &texts::group_mute(&group.name), muted);
    });
    response
}

#[derive(Default)]
pub struct MasterResponse {
    pub level: Option<f64>,
    pub mute: Option<bool>,
}

pub fn master(
    ui: &mut Ui,
    reference: Option<f64>,
    muted: Option<bool>,
    range: RangeInclusive<f64>,
    unit: &str,
) -> MasterResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = MasterResponse::default();
    frame(ui, tokens.accent, |ui| {
        ui.add(egui::Label::new(RichText::new(texts::MASTER).strong()));
        response.level = fader::shared(ui, texts::MASTER_LEVEL.get(), reference, range, unit);
        let readout = reference.map(texts::shared_readout).unwrap_or_default();
        ui.add(egui::Label::new(RichText::new(readout).monospace()));
        response.mute = mute_button(ui, texts::MUTE_ALL.get(), muted);
    });
    response
}

fn frame(ui: &mut Ui, color: Color32, contents: impl FnOnce(&mut Ui)) {
    let tokens = Tokens::current(ui.ctx());
    let inner_width = STRIP_WIDTH - 2.0 * (f32::from(STRIP_MARGIN) + BORDER_WIDTH);
    egui::Frame::new()
        .fill(tokens.card)
        .stroke(Stroke::new(BORDER_WIDTH, color))
        .corner_radius(CornerRadius::same(STRIP_RADIUS))
        .inner_margin(STRIP_MARGIN)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(inner_width);
                contents(ui);
            });
        });
}

/// Shows whether all are muted; a click asks for the opposite for all.
fn mute_button(ui: &mut Ui, label: &str, muted: Option<bool>) -> Option<bool> {
    let all = muted.unwrap_or(false);
    let button = ui.add_enabled(muted.is_some(), Button::new(texts::MUTE).selected(all));
    button.widget_info(|| WidgetInfo::selected(WidgetType::Button, muted.is_some(), all, label));
    button.clicked().then_some(!all)
}
