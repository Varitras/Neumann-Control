//! The "+ Adresse" dialog: one IPv6 address (link-local with `%index`), port fixed to 45.

use eframe::egui::{Id, Key, Modal, TextEdit, Ui};
use neumann_control_core::address::{parse_address, AddressError, SSC_PORT};
use neumann_control_core::settings::ManualSpeaker;

use crate::texts;

const MODAL_ID: &str = "add_address";
const FIELD_WIDTH: f32 = 260.0;

#[derive(Debug, Default)]
pub struct AddAddressForm {
    text: String,
    error: Option<AddressError>,
    focused_once: bool,
}

#[derive(Debug, PartialEq)]
pub enum AddAddressAction {
    None,
    Cancel,
    Add(ManualSpeaker),
}

pub fn show(ui: &mut Ui, form: &mut AddAddressForm) -> AddAddressAction {
    let modal = Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, form));
    if modal.should_close() {
        return AddAddressAction::Cancel;
    }
    modal.inner
}

fn contents(ui: &mut Ui, form: &mut AddAddressForm) -> AddAddressAction {
    ui.heading(texts::ADD_ADDRESS_TITLE);
    let label = ui.label(texts::ADDRESS_FIELD);
    let field = ui
        .horizontal(|ui| {
            let field = ui
                .add(
                    TextEdit::singleline(&mut form.text)
                        .hint_text(texts::ADDRESS_HINT)
                        .desired_width(FIELD_WIDTH),
                )
                .labelled_by(label.id);
            ui.label(texts::port(SSC_PORT));
            field
        })
        .inner;
    if !form.focused_once {
        form.focused_once = true;
        field.request_focus();
    }
    if field.changed() {
        form.error = None;
    }
    if let Some(error) = &form.error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            texts::describe_address_error(error),
        );
    }
    let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    let (added, cancelled) = ui
        .horizontal(|ui| {
            (
                ui.button(texts::ADD).clicked(),
                ui.button(texts::CANCEL).clicked(),
            )
        })
        .inner;
    if cancelled {
        return AddAddressAction::Cancel;
    }
    if !(added || submitted) {
        return AddAddressAction::None;
    }
    submit(form)
}

fn submit(form: &mut AddAddressForm) -> AddAddressAction {
    let text = form.text.trim();
    match parse_address(text, SSC_PORT) {
        Ok(_) => AddAddressAction::Add(ManualSpeaker {
            address: text.to_owned(),
            port: SSC_PORT,
            serial: None,
        }),
        Err(error) => {
            form.error = Some(error);
            AddAddressAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(text: &str) -> AddAddressForm {
        AddAddressForm {
            text: text.to_owned(),
            ..AddAddressForm::default()
        }
    }

    #[test]
    fn a_link_local_address_with_index_is_added_on_port_45_trimmed() {
        let mut form = form("  fe80::2a36:38ff:fe12:3456%7 ");
        assert_eq!(
            submit(&mut form),
            AddAddressAction::Add(ManualSpeaker {
                address: "fe80::2a36:38ff:fe12:3456%7".to_owned(),
                port: SSC_PORT,
                serial: None,
            })
        );
    }

    #[test]
    fn a_link_local_address_without_index_keeps_the_dialog_with_its_reason() {
        let mut form = form("fe80::2a36:38ff:fe12:3456");
        assert_eq!(submit(&mut form), AddAddressAction::None);
        assert_eq!(form.error, Some(AddressError::MissingScope));
    }
}
