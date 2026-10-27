//! The strip of a group (name, link buttons, group fader, mute) and the master strip at the end
//! of the desk. Both only report gestures; the app turns them into one write per speaker.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Button, Color32, CornerRadius, Rect, RichText, Role, Stroke, Ui, WidgetInfo,
};
use neumann_control_core::settings::{GroupSettings, Linkable};

use super::fader::{self, Orientation};
use super::layout_menu::{popup, right_clicked};
use super::named_region;
use super::strip::{BORDER_WIDTH, STRIP_MARGIN, STRIP_RADIUS};
use crate::groups::{linkable_label, SharedLevel, LINKABLE};
use crate::texts;
use crate::ui_kit::theme::Tokens;

const GROUP_MENU_ID: &str = "group_strip_menu";
const STRIP_WIDTH: f32 = 96.0;

#[derive(Default)]
pub struct GroupStripResponse {
    /// The move the group fader was let go at, in dB.
    pub level: Option<f64>,
    /// The mute every member should get.
    pub mute: Option<bool>,
    pub toggled_link: Option<Linkable>,
    pub open_settings: bool,
    /// From the strip's context menu: the groups window asks before deleting.
    pub delete: bool,
}

/// `level` follows the loudest connected member, `muted` says whether all of them are muted. The
/// strip is named after the group for screen readers.
pub fn group(
    ui: &mut Ui,
    group: &GroupSettings,
    color: Option<Color32>,
    level: Option<&SharedLevel>,
    muted: Option<bool>,
    range: RangeInclusive<f64>,
) -> GroupStripResponse {
    named_region(ui, &texts::strip_region(&group.name), |ui| {
        group_contents(ui, group, color, level, muted, range)
    })
}

fn group_contents(
    ui: &mut Ui,
    group: &GroupSettings,
    color: Option<Color32>,
    level: Option<&SharedLevel>,
    muted: Option<bool>,
    range: RangeInclusive<f64>,
) -> GroupStripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = GroupStripResponse::default();
    let rect = frame(ui, color.unwrap_or(tokens.border), |ui| {
        let mut name = RichText::new(&group.name).strong();
        if let Some(color) = color {
            name = name.color(color);
        }
        response.open_settings = ui.add(Button::new(name).frame(false).truncate()).clicked();
        ui.add(egui::Label::new(
            RichText::new(texts::LINKED).small().weak(),
        ));
        for linkable in LINKABLE {
            let text = linkable_label(linkable);
            let on = group.linked.contains(&linkable);
            // Wrapped: the panel's names ("Eingangs-Interface") are wider than the strip.
            let button = Button::new(RichText::new(text).small()).wrap().selected(on);
            let button = ui.add(button);
            let label = texts::link_label(text);
            button.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, &label));
            if button.clicked() {
                response.toggled_link = Some(linkable);
            }
        }
        let label = texts::group_level(&group.name);
        response.level = fader::shared(ui, &label, level, range, Orientation::Vertical);
        let readout = shared_readout(level);
        ui.add(egui::Label::new(RichText::new(readout).monospace()));
        response.mute = mute_button(ui, &texts::group_mute(&group.name), muted);
        let details = ui.button(texts::DETAILS);
        let label = texts::group_details(&group.name);
        details.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        response.open_settings |= details.clicked();
    });
    let menu = ui.scope_id().with((GROUP_MENU_ID, &group.name));
    popup(ui, menu, right_clicked(ui, rect), |ui| {
        response.delete = ui.button(texts::DELETE_GROUP_MENU).clicked();
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
    level: Option<&SharedLevel>,
    muted: Option<bool>,
    range: RangeInclusive<f64>,
) -> MasterResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = MasterResponse::default();
    frame(ui, tokens.accent, |ui| {
        ui.add(egui::Label::new(RichText::new(texts::MASTER).strong()));
        let label = texts::MASTER_LEVEL.get();
        response.level = fader::shared(ui, label, level, range, Orientation::Vertical);
        let readout = shared_readout(level);
        ui.add(egui::Label::new(RichText::new(readout).monospace()));
        response.mute = mute_button(ui, texts::MUTE_ALL.get(), muted);
    });
    response
}

pub(super) fn shared_readout(level: Option<&SharedLevel>) -> String {
    level
        .map(|level| texts::shared_readout(level.reference))
        .unwrap_or_default()
}

/// Draws a strip's frame around `contents`; returns where it lies.
pub(super) fn frame(ui: &mut Ui, color: Color32, contents: impl FnOnce(&mut Ui)) -> Rect {
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
        })
        .response
        .rect
}

/// Shows whether all are muted; a click asks for the opposite for all.
pub(super) fn mute_button(ui: &mut Ui, label: &str, muted: Option<bool>) -> Option<bool> {
    let all = muted.unwrap_or(false);
    let button = ui.add_enabled(muted.is_some(), Button::new(texts::MUTE).selected(all));
    button.widget_info(|| WidgetInfo::selected(Role::Button, muted.is_some(), all, label));
    button.clicked().then_some(!all)
}
