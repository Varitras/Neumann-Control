//! The controls of the "Mehr" area, one per writable kind: number, switch, selection and text.
//! Every write goes through `writes`.

use eframe::egui::{Button, Checkbox, ComboBox, DragValue, Event, Key, TextEdit, Ui};
use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use crate::state::SpeakerState;
use crate::texts;
use crate::writes::{self, Intent};

/// Drag speed for numbers without a step in the catalog, in units per point.
const UNSTEPPED_DRAG_SPEED: f64 = 0.1;
const PENDING_NUMBER_ID: &str = "pending_number";
const TEXT_DRAFT_ID: &str = "text_draft";
const TEXT_FIELD_ID: &str = "text_field";

fn is_text_edit(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Paste(_) | Event::Cut => true,
        Event::Key {
            key, pressed: true, ..
        } => matches!(key, Key::Backspace | Key::Delete),
        _ => false,
    }
}

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

    /// A drag or a typed value is one gesture. Its value is held while the gesture is active
    /// and written once, on the frame it ends (release, Enter or focus loss); Escape drops it.
    /// The device value itself is never clamped or written back. The field shows the value
    /// unrounded: egui parses the shown text when focus leaves and would otherwise report the
    /// rounded text as an edit.
    pub fn number(&mut self, ui: &mut Ui, entry: &Entry) {
        let reported = self.speaker.values.get(&entry.path).cloned();
        let device = reported.as_ref().and_then(Value::as_f64);
        let enabled = self.enabled(entry, device.is_some());
        let product = self.speaker.model().unwrap_or_default();
        let min = entry.min.unwrap_or(f64::NEG_INFINITY);
        let max = entry
            .max_for(product, &self.catalog.classes)
            .unwrap_or(f64::INFINITY);
        let pending_id = ui.id().with(entry.path.to_string()).with(PENDING_NUMBER_ID);
        let held = ui.data(|data| data.get_temp::<f64>(pending_id));
        let mut value = held.or(device).unwrap_or(entry.min.unwrap_or_default());
        let mut drag = DragValue::new(&mut value)
            .range(min..=max)
            .clamp_existing_to_range(false)
            .custom_formatter(|number, _| number.to_string())
            .speed(entry.step.unwrap_or(UNSTEPPED_DRAG_SPEED));
        if let Some(unit) = &entry.unit {
            drag = drag.suffix(format!(" {unit}"));
        }
        let response = ui
            .horizontal_wrapped(|ui| {
                let label = ui.label(&entry.label);
                ui.add_enabled(enabled, drag).labelled_by(label.id)
            })
            .inner;
        let active = response.dragged() || response.has_focus();
        let ended = response.drag_stopped() || response.lost_focus();
        if !active && !ended {
            ui.data_mut(|data| data.remove::<f64>(pending_id));
            return;
        }
        // egui also reports a change on the frame focus leaves, when it reparses and clamps
        // the shown text; that one is the user's only if the same frame carried their typing.
        let typed_this_frame = ui.input(|input| input.events.iter().any(is_text_edit));
        let changed_by_user = response.changed() && (active || typed_this_frame);
        if changed_by_user {
            ui.data_mut(|data| data.insert_temp(pending_id, value));
        }
        if active {
            return;
        }
        let Some(number) = changed_by_user.then_some(value).or(held) else {
            return;
        };
        ui.data_mut(|data| data.remove::<f64>(pending_id));
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            return;
        }
        let path = entry.path.clone();
        let value = writes::number_value(entry, reported.as_ref(), number);
        self.send(ui, Intent::Write { path, value });
    }

    pub fn switch(&mut self, ui: &mut Ui, entry: &Entry) {
        let current = self
            .speaker
            .values
            .get(&entry.path)
            .and_then(Value::as_bool);
        let enabled = self.enabled(entry, current.is_some());
        let mut checked = current.unwrap_or(false);
        let checkbox = Checkbox::new(&mut checked, &entry.label);
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
        let label = ui.label(&entry.label);
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
        let draft_id = ui.id().with(entry.path.to_string()).with(TEXT_DRAFT_ID);
        let field_id = draft_id.with(TEXT_FIELD_ID);
        let mut draft = ui
            .data(|data| data.get_temp::<TextDraft>(draft_id))
            .unwrap_or_default();
        let focused = ui.memory(|memory| memory.has_focus(field_id));
        if !draft.edited && !focused {
            draft.text = reported.unwrap_or_default();
        }
        let label = ui.label(&entry.label);
        let field = TextEdit::singleline(&mut draft.text)
            .id(field_id)
            .desired_width(f32::INFINITY);
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
