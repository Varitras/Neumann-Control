//! The card's expanded area: settings per model, displays, confirmation and number gestures.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::{json, Value};
use support::*;

const STANDBY_LABEL: &str = "Auto-Standby";
const DELAY_LABEL: &str = "Verzögerung";
const CONTROL_MODE_LABEL: &str = "Steuermodus";
const NAME_PATH: &str = "/device/name";
const CONTROL_MODE_PATH: &str = "/ui/control_mode";
const DELAY_PATH: &str = "/audio/out/delay";
const STANDBY_TIME_LABEL: &str = "Standby-Zeit";
const STANDBY_TIME_PATH: &str = "/device/standby/auto_standby_time";
/// Frames a widget would need to write a value nobody chose, if it did.
const IDLE_FRAMES: usize = 10;

/// A connected card with "Mehr" open, in a window tall enough for the whole area.
fn expanded_card(model: SimModel) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_card(model);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.get_by_label(texts::MORE).click();
    wait_until(&mut harness, "the expanded area", |harness| {
        harness.query_by_label(texts::ALL_VALUES).is_some()
    });
    harness.run_steps(1);
    (simulator, directory, harness)
}

/// The values written to `pointer`; polls ask with `null` and are left out.
fn sets_to(simulator: &Simulator, pointer: &str) -> Vec<Value> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| request.pointer(pointer).filter(|value| !value.is_null()))
        .cloned()
        .collect()
}

fn role_of(harness: &Harness<'static, NeumannApp>, label: &str) -> Role {
    harness.get_by_label(label).accesskit_node().role()
}

#[test]
fn kh750_details_show_out1_and_no_phase() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh750);
    harness.get_by_label("Ausgang 1");
    harness.get_by_label("Ausgang 2");
    assert!(harness.query_by_label("Phase invertiert").is_none());
}

#[test]
fn kh750_standby_enabled_is_display_only() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh750);
    assert_eq!(role_of(&harness, STANDBY_LABEL), Role::Label);
    assert!(harness.query_all_by_role(Role::CheckBox).all(|node| node
        .accesskit_node()
        .label()
        .as_deref()
        != Some(STANDBY_LABEL)));
}

#[test]
fn kh120ii_standby_enabled_is_checkbox() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    assert_eq!(role_of(&harness, STANDBY_LABEL), Role::CheckBox);
    harness.get_by_label(STANDBY_LABEL).click();
    wait_until(&mut harness, "standby off on the device", |_| {
        simulator.value("device/standby/enabled") == Some(Value::Bool(false))
    });
}

#[test]
fn temperature_is_shown_in_celsius() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh750);
    simulator.set_value("device/temperature", json!(313));
    wait_until(&mut harness, "40 °C", |harness| {
        harness.query_by_label("40 °C").is_some()
    });
}

#[test]
fn name_field_shows_refusal() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Küche");
    harness.run_steps(1);
    harness.get_by_label(texts::APPLY).click();
    harness.run_steps(2);
    barrier(&simulator);
    let sets = sets_to(&simulator, NAME_PATH);
    assert!(sets.is_empty(), "{sets:?}");
    let refusal = texts::describe_value_error(&ValueError::NotPrintableAscii);
    let refusal = harness.get_by_label(&refusal).rect();
    let field = harness.get_by_role(Role::TextInput);
    assert_eq!(field.value().as_deref(), Some("Küche"));
    assert!(
        refusal.min.y > field.rect().max.y,
        "the reason is below the field"
    );
    let next_row = harness.get_by_label(texts::ALL_VALUES).rect();
    assert!(
        refusal.max.y < next_row.min.y,
        "the reason is not the card's message"
    );
}

#[test]
fn local_control_mode_needs_confirmation() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_label(CONTROL_MODE_LABEL).click();
    harness.run_steps(2);
    harness.get_by_label("LOCAL").click();
    harness.run_steps(2);
    barrier(&simulator);
    assert!(sets_to(&simulator, CONTROL_MODE_PATH).is_empty());
    harness.get_by_label(texts::LOCAL_MODE_WARNING);
    harness.get_by_label(texts::SWITCH_MODE).click();
    wait_until(&mut harness, "LOCAL on the device", |_| {
        simulator.value("ui/control_mode") == Some(json!("LOCAL"))
    });
    assert_eq!(sets_to(&simulator, CONTROL_MODE_PATH), vec![json!("LOCAL")]);
}

#[test]
fn number_field_sends_once_per_gesture() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    let start = harness.get_by_label(DELAY_LABEL).rect().center();
    harness.drag_at(start);
    for frame in 1..=10 {
        harness.hover_at(start + egui::vec2(4.0 * frame as f32, 0.0));
        harness.step();
    }
    barrier(&simulator);
    assert!(
        sets_to(&simulator, DELAY_PATH).is_empty(),
        "a Set went out while dragging"
    );
    harness.drop_at(start + egui::vec2(40.0, 0.0));
    wait_until(&mut harness, "the Set after release", |_| {
        !sets_to(&simulator, DELAY_PATH).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = sets_to(&simulator, DELAY_PATH);
    assert_eq!(sets.len(), 1, "{sets:?}");
    let delay = sets[0]
        .as_i64()
        .expect("the delay is sent as a whole number");
    assert!(delay > 0, "dragging right raises the delay: {sets:?}");
}

/// Clicks a number field into text editing and types `text` over its value.
fn type_number(harness: &mut Harness<'static, NeumannApp>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    harness.run_steps(1);
    harness.get_by_label(label).type_text(text);
    harness.run_steps(1);
}

fn numeric_value(harness: &Harness<'static, NeumannApp>, label: &str) -> Option<f64> {
    harness.get_by_label(label).accesskit_node().numeric_value()
}

#[test]
fn out_of_range_device_value_is_not_written_back() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(6000.0)
    });
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets_to(&simulator, DELAY_PATH);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn abandoned_number_edit_is_not_sent_on_reopen() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50");
    harness.get_by_label(texts::LESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets_to(&simulator, STANDBY_TIME_PATH);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn a_typed_number_is_snapped_only_when_sent() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    type_number(&mut harness, STANDBY_TIME_LABEL, "1.5");
    harness.run_steps(2);
    let shown = harness.get_by_label(STANDBY_TIME_LABEL).value();
    assert_eq!(shown.as_deref(), Some("1.5"));
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the Set after Enter", |_| {
        !sets_to(&simulator, STANDBY_TIME_PATH).is_empty()
    });
    assert_eq!(sets_to(&simulator, STANDBY_TIME_PATH), vec![json!(2)]);
}

#[test]
fn escape_during_number_edit_sends_nothing() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets_to(&simulator, STANDBY_TIME_PATH);
    assert!(sets.is_empty(), "{sets:?}");
    assert_eq!(numeric_value(&harness, STANDBY_TIME_LABEL), Some(60.0));
}

#[test]
fn an_abandoned_name_draft_follows_the_device_again() {
    let (_simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.get_by_role(Role::TextInput).type_text("draft");
    harness.run_steps(1);
    let typed = harness
        .get_by_role(Role::TextInput)
        .value()
        .unwrap_or_default();
    assert!(typed.contains("draft"), "{typed}");
    harness.get_by_label("Gerät").click();
    harness.run_steps(2);
    let shown = harness.get_by_role(Role::TextInput).value();
    assert_eq!(shown.as_deref(), Some("Simulated KH 120 II"));
}

#[test]
fn card_controls_are_not_repeated_in_the_area() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh120Ii);
    assert_eq!(harness.query_all_by_label(texts::MUTE).count(), 1);
    assert_eq!(harness.query_all_by_label(texts::IDENTIFY).count(), 1);
    assert!(harness.query_by_label("Identifizieren").is_none());
    assert!(harness.query_by_label("Eingangspegel").is_none());
    assert_eq!(harness.query_all_by_role(Role::Slider).count(), 1);
    assert!(harness.query_all_by_role(Role::SpinButton).all(|node| node
        .accesskit_node()
        .label()
        .as_deref()
        != Some(texts::LEVEL)));
}

#[test]
fn kh750_output_meters_are_drawn() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh750);
    // One for the meters, one for the rear-panel output level.
    assert_eq!(harness.query_all_by_label("Ausgangspegel").count(), 2);
}

#[test]
fn cancelling_local_control_mode_sends_nothing() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_label(CONTROL_MODE_LABEL).click();
    harness.run_steps(2);
    harness.get_by_label("LOCAL").click();
    harness.run_steps(2);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    assert!(sets_to(&simulator, CONTROL_MODE_PATH).is_empty());
    assert!(harness.query_by_label(texts::LOCAL_MODE_WARNING).is_none());
}

#[test]
fn network_control_mode_is_sent_without_confirmation() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    simulator.set_value("ui/control_mode", json!("LOCAL"));
    wait_until(&mut harness, "LOCAL on the card", |harness| {
        harness.get_by_label(CONTROL_MODE_LABEL).value().as_deref() == Some("LOCAL")
    });
    harness.get_by_label(CONTROL_MODE_LABEL).click();
    harness.run_steps(2);
    harness.get_by_label("NETWORK").click();
    wait_until(&mut harness, "NETWORK on the device", |_| {
        simulator.value("ui/control_mode") == Some(json!("NETWORK"))
    });
    assert!(harness.query_by_label(texts::LOCAL_MODE_WARNING).is_none());
}

#[test]
fn all_values_button_reads_the_whole_tree() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::ALL_VALUES).click();
    wait_until(&mut harness, "the schema walk", |_| {
        simulator
            .requests()
            .iter()
            .any(|request| request.pointer("/osc/schema").is_some())
    });
}
