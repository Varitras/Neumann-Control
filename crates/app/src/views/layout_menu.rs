//! The context menus that change a model's strip layout: on an element of a strip, and on a row
//! of the device panel. A right click anywhere on the element's area opens them, also on a
//! disabled control, which takes no click itself. They only report the change.

use eframe::egui::{Id, Popup, PopupAnchor, PopupKind, Rect, SetOpenCommand, Ui};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::StripElement;

use crate::strip_layout::{self, Area, LayoutChange, StripLayout};
use crate::texts;

const ELEMENT_MENU_ID: &str = "strip_element_menu";
const ROW_MENU_ID: &str = "panel_row_menu";

/// An element that could still go on the strip, with the name it is offered under.
pub struct Offer {
    pub element: StripElement,
    pub label: String,
}

/// What `product`'s strips could still take, named as in the device panel.
pub fn offers(layout: &StripLayout, product: &str, catalog: &Catalog) -> Vec<Offer> {
    strip_layout::addable(layout, product, catalog)
        .into_iter()
        .map(|element| Offer {
            label: element_label(&element, catalog),
            element,
        })
        .collect()
}

pub(super) fn element_label(element: &StripElement, catalog: &Catalog) -> String {
    let StripElement::Path(path) = element else {
        return texts::EQ.get().to_owned();
    };
    catalog
        .entries
        .iter()
        .find(|entry| entry.path == *path)
        .map_or_else(
            || path.to_string(),
            |entry| entry.label(texts::language()).to_owned(),
        )
}

/// Hide, swap, move up or down one element of a strip.
pub fn element_menu(
    ui: &Ui,
    rect: Rect,
    element: &StripElement,
    offers: &[Offer],
) -> Option<LayoutChange> {
    let id = ui.scope_id().with((ELEMENT_MENU_ID, menu_key(element)));
    let mut change = None;
    menu(ui, id, rect, |ui| {
        if ui.button(texts::HIDE).clicked() {
            change = Some(LayoutChange::Remove(element.clone()));
        }
        ui.add_enabled_ui(!offers.is_empty(), |ui| {
            ui.menu_button(texts::SWAP_FOR, |ui| {
                for offer in offers {
                    if ui.button(&offer.label).clicked() {
                        change = Some(LayoutChange::Swap {
                            old: element.clone(),
                            new: offer.element.clone(),
                        });
                    }
                }
            });
        });
        if ui.button(texts::MOVE_UP).clicked() {
            change = Some(LayoutChange::MoveUp(element.clone()));
        }
        if ui.button(texts::MOVE_DOWN).clicked() {
            change = Some(LayoutChange::MoveDown(element.clone()));
        }
    });
    change
}

/// Put a panel row on the strip (below the fader), or take it off.
pub fn row_menu(
    ui: &Ui,
    rect: Rect,
    element: &StripElement,
    on_strip: bool,
) -> Option<LayoutChange> {
    let id = ui.scope_id().with((ROW_MENU_ID, menu_key(element)));
    let mut change = None;
    menu(ui, id, rect, |ui| {
        let (text, wish) = match on_strip {
            true => (
                texts::REMOVE_FROM_STRIP,
                LayoutChange::Remove(element.clone()),
            ),
            false => (
                texts::ADD_TO_STRIP,
                LayoutChange::Add(element.clone(), Area::Below),
            ),
        };
        if ui.button(text).clicked() {
            change = Some(wish);
        }
    });
    change
}

fn menu(ui: &Ui, id: Id, rect: Rect, contents: impl FnOnce(&mut Ui)) {
    let right_clicked =
        ui.input(|input| input.pointer.secondary_clicked()) && ui.rect_contains_pointer(rect);
    Popup::new(
        id,
        ui.ctx().clone(),
        PopupAnchor::PointerFixed,
        ui.layer_id(),
    )
    .kind(PopupKind::Menu)
    .open_memory(right_clicked.then_some(SetOpenCommand::Bool(true)))
    .show(contents);
}

pub(super) fn menu_key(element: &StripElement) -> String {
    match element {
        StripElement::Path(path) => path.to_string(),
        StripElement::Eq => texts::EQ.get().to_owned(),
    }
}
