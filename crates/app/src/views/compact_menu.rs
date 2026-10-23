//! The context menus that change a model's compact buttons: on a button, and on the rest of the
//! row, which still offers adding when every button is hidden. They only report the change.

use eframe::egui::{Rect, Ui};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::CompactButton;

use super::layout_menu::{popup, right_clicked};
use crate::compact_layout::{self, CompactChange};
use crate::paths;
use crate::texts;

const BUTTON_MENU_ID: &str = "compact_button_menu";
const ROW_MENU_ID: &str = "compact_row_menu";

/// A button that could still go on the row, with the name it is offered under.
pub struct CompactOffer {
    pub button: CompactButton,
    pub label: String,
}

/// What `product`'s rows could still take, switches named as in the device panel.
pub fn offers(buttons: &[CompactButton], product: &str, catalog: &Catalog) -> Vec<CompactOffer> {
    compact_layout::addable(buttons, product, catalog)
        .into_iter()
        .map(|button| CompactOffer {
            label: offer_label(&button, catalog),
            button,
        })
        .collect()
}

fn offer_label(button: &CompactButton, catalog: &Catalog) -> String {
    match button {
        CompactButton::Eq => texts::EQ.get().to_owned(),
        CompactButton::Details => texts::DETAILS.get().to_owned(),
        CompactButton::Switch(path) => paths::catalog_entry(catalog, path).map_or_else(
            || path.to_string(),
            |entry| entry.label(texts::language()).to_owned(),
        ),
    }
}

/// Hide or move one button, add another, or go back to the default buttons.
pub fn button_menu(
    ui: &Ui,
    rect: Rect,
    button: &CompactButton,
    offers: &[CompactOffer],
) -> Option<CompactChange> {
    let id = ui.scope_id().with((BUTTON_MENU_ID, format!("{button:?}")));
    let mut change = None;
    popup(ui, id, right_clicked(ui, rect), |ui| {
        let moves = [
            (texts::HIDE, CompactChange::Remove(button.clone())),
            (texts::MOVE_LEFT, CompactChange::MoveLeft(button.clone())),
            (texts::MOVE_RIGHT, CompactChange::MoveRight(button.clone())),
        ];
        for (text, wish) in moves {
            if ui.button(text).clicked() {
                change = Some(wish);
            }
        }
        if let Some(wish) = add_and_reset(ui, offers) {
            change = Some(wish);
        }
    });
    change
}

/// Right-clicked in `row` but on none of its `buttons`: add a button or go back to the default.
pub fn row_menu(
    ui: &Ui,
    row: Rect,
    buttons: &[Rect],
    offers: &[CompactOffer],
) -> Option<CompactChange> {
    let on_a_button = buttons.iter().any(|rect| ui.rect_contains_pointer(*rect));
    let open = right_clicked(ui, row) && !on_a_button;
    let mut change = None;
    popup(ui, ui.scope_id().with(ROW_MENU_ID), open, |ui| {
        change = add_and_reset(ui, offers);
    });
    change
}

fn add_and_reset(ui: &mut Ui, offers: &[CompactOffer]) -> Option<CompactChange> {
    let mut change = None;
    ui.add_enabled_ui(!offers.is_empty(), |ui| {
        ui.menu_button(texts::ADD_BUTTON, |ui| {
            for offer in offers {
                if ui.button(&offer.label).clicked() {
                    change = Some(CompactChange::Add(offer.button.clone()));
                }
            }
        });
    });
    if ui.button(texts::DEFAULT_LAYOUT).clicked() {
        change = Some(CompactChange::Reset);
    }
    change
}
