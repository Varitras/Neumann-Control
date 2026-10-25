//! The device panel's controls, one per writable kind: number, switch, selection and text.
//! Every write goes through `writes`.

use std::ops::RangeInclusive;

use eframe::egui::{Button, Checkbox, ComboBox, Key, TextEdit, Ui};
use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::number_field::{self, NumberField};
use crate::display::{capped, conversion_of};
use crate::state::SpeakerState;
use crate::texts;
use crate::writes::{self, Intent};

/// Drag speed for numbers without a step in the catalog, in units per point.
const UNSTEPPED_DRAG_SPEED: f64 = 0.1;
const TEXT_DRAFT_ID: &str = "text_draft";
const TEXT_FIELD_ID: &str = "text_field";

/// What the user typed into a text field and has not yet written.
#[derive(Clone, Debug, Default)]
struct TextDraft {
    text: String,
    edited: bool,
    refusal: Option<String>,
}

pub(super) struct Fields<'a> {
    pub speaker: &'a mut SpeakerState,
    pub catalog: &'a Catalog,
    pub commands: &'a mut Vec<Command>,
}

impl Fields<'_> {
    fn send(&mut self, ui: &Ui, intent: Intent) {
        let now = ui.input(|input| input.time);
        let command = self.speaker.request(intent, self.catalog, now);
        self.commands.extend(command);
    }

    fn enabled(&self, entry: &Entry, known: bool) -> bool {
        known && writes::can_write(self.speaker, self.catalog, &entry.path)
    }

    /// One gesture per write, see `number_field`; a typed value outside the range is sent as
    /// typed, for the write policy to refuse with the reason.
    pub fn number(&mut self, ui: &mut Ui, entry: &Entry) {
        let reported = self.speaker.values.get(&entry.path).cloned();
        let device = reported.as_ref().and_then(Value::as_f64);
        let product = self.speaker.product();
        let min = entry.min.unwrap_or(f64::NEG_INFINITY);
        let max = entry
            .max_for(product, &self.catalog.classes)
            .unwrap_or(f64::INFINITY);
        let written = ui
            .horizontal_wrapped(|ui| {
                let label = ui.label(entry.label(texts::language()));
                let field = NumberField {
                    id: ui.scope_id().with(entry.path.to_string()),
                    device,
                    range: min..=max,
                    step: entry.step,
                    drag_speed: entry.step.unwrap_or(UNSTEPPED_DRAG_SPEED),
                    unit: entry.unit.as_deref(),
                    conversion: conversion_of(entry),
                    enabled: self.enabled(entry, device.is_some()),
                    labelled_by: Some(label.id),
                };
                number_field::show(ui, &field)
            })
            .inner;
        let Some(number) = written else {
            return;
        };
        let value = field_value(entry, reported.as_ref(), number, min..=max);
        let path = entry.path.clone();
        self.send(ui, Intent::Write { path, value });
    }

    /// A list value (`elements` in the catalog): one field per entry. A change writes the whole
    /// list, the other entries as last sent or else as the speaker reported them, so a second
    /// change before the answer keeps the first.
    pub fn number_list(&mut self, ui: &mut Ui, entry: &Entry) {
        let names = entry.element_labels(texts::language());
        let current: Option<Vec<Value>> = self
            .speaker
            .latest(&entry.path)
            .and_then(Value::as_array)
            .filter(|entries| entries.len() == names.len())
            .cloned();
        let product = self.speaker.product();
        let min = entry.min.unwrap_or(f64::NEG_INFINITY);
        let max = entry
            .max_for(product, &self.catalog.classes)
            .unwrap_or(f64::INFINITY);
        let mut written = None;
        ui.label(entry.label(texts::language()));
        for (index, name) in names.iter().enumerate() {
            ui.horizontal(|ui| {
                let device = current.as_ref().and_then(|entries| entries[index].as_f64());
                let label = ui.label(name);
                let field = NumberField {
                    id: ui.scope_id().with(entry.path.to_string()).with(index),
                    device,
                    range: min..=max,
                    step: entry.step,
                    drag_speed: entry.step.unwrap_or(UNSTEPPED_DRAG_SPEED),
                    unit: entry.unit.as_deref(),
                    conversion: conversion_of(entry),
                    enabled: self.enabled(entry, device.is_some()),
                    labelled_by: Some(label.id),
                };
                if let Some(number) = number_field::show(ui, &field) {
                    written = Some((index, number));
                }
            });
        }
        let (Some((index, number)), Some(mut entries)) = (written, current) else {
            return;
        };
        entries[index] = field_value(entry, entries.get(index), number, min..=max);
        let path = entry.path.clone();
        let value = Value::Array(entries);
        self.send(ui, Intent::Write { path, value });
    }

    /// Shows and toggles from the value last sent, as the strip does: a second click before the
    /// answer undoes the first instead of repeating it.
    pub fn switch(&mut self, ui: &mut Ui, entry: &Entry) {
        let current = self.speaker.latest(&entry.path).and_then(Value::as_bool);
        let enabled = self.enabled(entry, current.is_some());
        let mut checked = current.unwrap_or(false);
        let checkbox = Checkbox::new(&mut checked, entry.label(texts::language()));
        if ui.add_enabled(enabled, checkbox).clicked() {
            let path = entry.path.clone();
            let value = Value::Bool(checked);
            self.send(ui, Intent::Write { path, value });
        }
    }

    pub fn select(&mut self, ui: &mut Ui, entry: &Entry) {
        let current = self
            .speaker
            .values
            .get(&entry.path)
            .and_then(Value::as_str)
            .map(str::to_owned);
        let enabled = self.enabled(entry, current.is_some());
        let current = current.unwrap_or_default();
        let mut chosen = None;
        let label = ui.label(entry.label(texts::language()));
        ui.add_enabled_ui(enabled, |ui| {
            ComboBox::from_id_salt(entry.path.to_string())
                .selected_text(&current)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for option in &entry.options {
                        let selected = *option == current;
                        if ui.selectable_label(selected, option).clicked() && !selected {
                            chosen = Some(option.clone());
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let Some(option) = chosen else {
            return;
        };
        let intent = Intent::Write {
            path: entry.path.clone(),
            value: Value::String(option),
        };
        self.send(ui, intent);
    }

    /// Written with "Übernehmen" or Enter after the catalog check; a refused text stays in
    /// the field with the reason below it. Polling never replaces what is being edited.
    pub fn text(&mut self, ui: &mut Ui, entry: &Entry) {
        let reported = self
            .speaker
            .values
            .get(&entry.path)
            .and_then(Value::as_str)
            .map(str::to_owned);
        let enabled = self.enabled(entry, reported.is_some());
        let draft_id = ui
            .scope_id()
            .with(entry.path.to_string())
            .with(TEXT_DRAFT_ID);
        let field_id = draft_id.with(TEXT_FIELD_ID);
        let mut draft = ui
            .data(|data| data.get_temp::<TextDraft>(draft_id))
            .unwrap_or_default();
        let focused = ui.memory(|memory| memory.has_focus(field_id));
        if !draft.edited && !focused {
            draft.text = reported.as_deref().map(capped).unwrap_or_default();
        }
        let label = ui.label(entry.label(texts::language()));
        let field = TextEdit::singleline(&mut draft.text)
            .id(field_id)
            .desired_width(ui.available_width());
        let field = ui.add_enabled(enabled, field).labelled_by(label.id);
        if field.changed() {
            draft.edited = true;
            draft.refusal = None;
        }
        // Keys typed while a dialog is open belong to the dialog.
        let dialog_open = ui.memory(|memory| memory.top_modal_layer().is_some());
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        let apply = ui.add_enabled(enabled, Button::new(texts::APPLY));
        if apply.clicked() || (entered && !dialog_open) {
            self.submit_text(ui, entry, &mut draft);
        }
        // The draft belongs to the field and "Übernehmen" together: only when focus has left
        // both (and no press is on the button) is it given back to the device, unless a
        // refusal still explains the text.
        let in_use = field.has_focus() || apply.has_focus() || apply.is_pointer_button_down_on();
        if !in_use && draft.refusal.is_none() {
            draft.edited = false;
        }
        if let Some(refusal) = &draft.refusal {
            ui.colored_label(ui.visuals().error_fg_color, refusal);
        }
        ui.data_mut(|data| data.insert_temp(draft_id, draft));
    }

    fn submit_text(&mut self, ui: &Ui, entry: &Entry, draft: &mut TextDraft) {
        let intent = Intent::Write {
            path: entry.path.clone(),
            value: Value::String(draft.text.clone()),
        };
        let now = ui.input(|input| input.time);
        match self.speaker.try_request(intent, self.catalog, now) {
            Ok(command) => {
                draft.edited = false;
                draft.refusal = None;
                self.commands.push(command);
            }
            Err(refusal) => draft.refusal = Some(texts::describe_refusal(&refusal)),
        }
    }
}

/// A typed number on the entry's grid and in the number type the speaker reports; outside the
/// range it goes as typed, for the write policy to refuse with the reason.
fn field_value(
    entry: &Entry,
    reported: Option<&Value>,
    number: f64,
    range: RangeInclusive<f64>,
) -> Value {
    match range.contains(&number) {
        true => writes::number_value(entry, reported, number),
        false => Value::from(number),
    }
}
