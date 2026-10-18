//! The card's expanded area: settings per model, displays and number gestures.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::discovery::Found;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::{json, Value};
use support::*;

const STANDBY_LABEL: &str = "Auto-Standby";
const DELAY_LABEL: &str = "Verzögerung";
const CONTROL_MODE_LABEL: &str = "Steuermodus";
const LOGO_LABEL: &str = "Logo-Helligkeit";
const NAME_PATH: &str = "/device/name";
const DELAY_PATH: &str = "/audio/out/delay";
const STANDBY_TIME_LABEL: &str = "Standby-Zeit";
const STANDBY_TIME_PATH: &str = "/device/standby/auto_standby_time";
/// Frames a widget would need to write a value nobody chose, if it did.
const IDLE_FRAMES: usize = 10;

/// A connected card with "Mehr" open, in a window tall enough for the whole area.
fn expanded_card(model: SimModel) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_card(model);
    open_panel(&mut harness, device_name(model));
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
    harness.get_by_label(texts::APPLY.get()).click();
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
    let next_row = harness.get_by_label(texts::ALL_VALUES.get()).rect();
    assert!(
        refusal.max.y < next_row.min.y,
        "the reason is not the card's message"
    );
}

#[test]
fn control_mode_is_shown_read_only_on_both_models() {
    for model in [SimModel::Kh120Ii, SimModel::Kh750] {
        let (_simulator, _directory, harness) = expanded_card(model);
        let combo_boxes = harness.query_all_by_role(Role::ComboBox);
        let labels: Vec<_> = combo_boxes
            .map(|node| node.accesskit_node().label())
            .collect();
        let offered = labels
            .iter()
            .any(|label| label.as_deref() == Some(CONTROL_MODE_LABEL));
        assert!(!offered, "{model:?} offers a control mode selection");
        assert_eq!(
            role_of(&harness, CONTROL_MODE_LABEL),
            Role::Label,
            "{model:?}"
        );
        harness.get_by_label("NETWORK");
    }
}

#[test]
fn an_announced_model_gives_no_write_controls_or_model_rows() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_value("device/identity/product", json!(""));
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let found = Found {
        name: String::new(),
        product: Some("KH 120 II".to_owned()),
        serial: Some(SIMULATED_SERIAL.to_owned()),
        address: simulator.address(),
    };
    let mut harness = harness(path, vec![found]);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    // Without an announced name the dialog names the speaker by its announced model.
    scan_and_add(&mut harness, &["KH 120 II"]);
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &simulator);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    assert_eq!(role_of(&harness, STANDBY_LABEL), Role::Label);
    assert!(harness.query_by_label(LOGO_LABEL).is_none());
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
fn visiting_a_number_field_does_not_write_its_rounded_text() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    simulator.set_value("audio/out/delay", json!(100.123456));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(100.123456)
    });
    harness.get_by_label(DELAY_LABEL).click();
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets_to(&simulator, DELAY_PATH);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn visiting_an_out_of_range_number_field_writes_nothing() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(6000.0)
    });
    harness.get_by_label(DELAY_LABEL).click();
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets_to(&simulator, DELAY_PATH);
    assert!(sets.is_empty(), "{sets:?}");
    assert_eq!(numeric_value(&harness, DELAY_LABEL), Some(6000.0));
}

#[test]
fn a_number_typed_and_entered_in_one_frame_is_sent() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_label(STANDBY_TIME_LABEL).click();
    harness.run_steps(2);
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::Text("50".to_owned()));
    events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    assert_eq!(sets_to(&simulator, STANDBY_TIME_PATH), vec![json!(50)]);
}

#[test]
fn abandoned_number_edit_is_not_sent_on_reopen() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50");
    // The first Escape cancels the edit, the second closes the panel.
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::CLOSE.get()).is_none());
    open_panel(&mut harness, KH120_DEVICE_NAME);
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
fn a_name_typed_then_tabbed_to_apply_is_sent() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Studio");
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the Set after Enter on Übernehmen", |_| {
        !sets_to(&simulator, NAME_PATH).is_empty()
    });
    barrier(&simulator);
    assert_eq!(sets_to(&simulator, NAME_PATH), vec![json!("Studio")]);
}

#[test]
fn a_name_draft_left_by_tabbing_past_apply_follows_the_device_again() {
    let (_simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.get_by_role(Role::TextInput).type_text("draft");
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(3);
    let shown = harness.get_by_role(Role::TextInput).value();
    assert_eq!(shown.as_deref(), Some("Simulated KH 120 II"));
}

#[test]
fn strip_controls_are_not_repeated_in_the_panel() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh120Ii);
    assert_eq!(harness.query_all_by_label(texts::MUTE.get()).count(), 1);
    assert_eq!(harness.query_all_by_label(texts::IDENTIFY.get()).count(), 1);
    assert!(harness.query_by_label("Identifizieren").is_none());
    assert!(harness.query_by_label("Eingangspegel").is_none());
    let faders = harness.query_all_by_role_and_label(Role::Slider, texts::LEVEL.get());
    assert_eq!(faders.count(), 1);
    assert!(harness.query_all_by_role(Role::SpinButton).all(|node| node
        .accesskit_node()
        .label()
        .as_deref()
        != Some(texts::LEVEL.get())));
}

#[test]
fn kh750_output_meters_sit_next_to_the_input_meters() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh750);
    let output_meter = texts::meter_label(texts::OUTPUT_METERS.get(), "A");
    wait_for_label(&mut harness, &output_meter);
    assert_eq!(
        harness
            .query_all_by_label_contains(texts::OUTPUT_METERS.get())
            .count(),
        3
    );
    assert_eq!(
        harness
            .query_all_by_label_contains(texts::INPUT_METERS.get())
            .count(),
        2
    );
    open_panel(&mut harness, KH750_DEVICE_NAME);
    assert_eq!(harness.query_all_by_label(&output_meter).count(), 1);
}

#[test]
fn kh120ii_card_has_no_output_meters() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    wait_for_label(
        &mut harness,
        &texts::meter_label(texts::INPUT_METERS.get(), "B"),
    );
    assert_eq!(
        harness
            .query_all_by_label_contains(texts::OUTPUT_METERS.get())
            .count(),
        0
    );
}

#[test]
fn all_values_button_reads_the_whole_tree() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::ALL_VALUES.get()).click();
    wait_until(&mut harness, "the schema walk", |_| {
        simulator
            .requests()
            .iter()
            .any(|request| request.pointer("/osc/schema").is_some())
    });
}

#[test]
fn panel_number_rows_keep_their_labels_on_one_line() {
    let (_simulator, _directory, harness) = expanded_card(SimModel::Kh120Ii);
    for label in [STANDBY_TIME_LABEL, LOGO_LABEL, DELAY_LABEL] {
        let rect = harness.get_by_role_and_label(Role::Label, label).rect();
        assert!(rect.width() > rect.height(), "{label} wraps: {rect:?}");
    }
    // The number field shows the value; the knob beside it does not repeat it, and the strip
    // shows a whole number as a whole number.
    assert!(harness.query_by_label_contains("60.0 min").is_none());
}

#[test]
fn delay_is_typed_in_milliseconds_and_sent_in_samples() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    type_number(&mut harness, DELAY_LABEL, "10");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the delay Set", |_| {
        !sets_to(&simulator, DELAY_PATH).is_empty()
    });
    assert_eq!(sets_to(&simulator, DELAY_PATH), vec![json!(480)]);
}

#[test]
fn visiting_a_delay_in_milliseconds_writes_nothing() {
    let (simulator, _directory, mut harness) = expanded_card(SimModel::Kh120Ii);
    // One sample is 0.0208333… ms, which no decimal text holds exactly.
    simulator.set_value("audio/out/delay", json!(1));
    wait_until(&mut harness, "the reported delay", |harness| {
        harness.query_by_label_contains("0.02").is_some()
    });
    harness.get_by_label(DELAY_LABEL).click();
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    assert!(sets_to(&simulator, DELAY_PATH).is_empty());
}
