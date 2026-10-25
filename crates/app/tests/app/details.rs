//! The device panel: settings per model, displays and number gestures.

use crate::support::*;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use neumann_control_app::app::NeumannApp;
use neumann_control_app::panel_tabs::PanelTab;
use neumann_control_app::paths;
use neumann_control_app::texts;
use neumann_control_app::writes::Refusal;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::SettingsPreset;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::{json, Value};

const STANDBY_LABEL: &str = "Auto-Standby";
const CONTROL_MODE_LABEL: &str = "Steuermodus";
const NAME_POINTER: &str = "/device/name";
const DELAY_POINTER: &str = "/audio/out/delay";
const STANDBY_TIME_POINTER: &str = "/device/standby/auto_standby_time";
/// Frames a widget would need to write a value nobody chose, if it did.
const IDLE_FRAMES: usize = 10;

/// A connected speaker with its device panel open on `tab`, in a window tall enough for all of it.
fn speaker_with_panel(
    model: SimModel,
    tab: PanelTab,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_speaker(model);
    open_panel_on(&mut harness, device_name(model), tab);
    (simulator, directory, harness)
}

/// The name field sits in "Device", below the outputs, input and standby: scrolled to first.
fn click_name_field(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_role(Role::TextInput).scroll_to_me();
    harness.run_steps(SCROLL_FRAMES);
    harness.get_by_role(Role::TextInput).click();
}

fn role_of(harness: &Harness<'static, NeumannApp>, label: &str) -> Role {
    harness.get_by_label(label).accesskit_node().role()
}

#[test]
fn kh750_details_show_out1_and_no_phase() {
    let (_simulator, _directory, harness) = speaker_with_panel(SimModel::Kh750, PanelTab::Outputs);
    harness.get_by_label("Ausgang A");
    harness.get_by_label("Ausgang B");
    assert!(harness.query_by_label("Phase invertiert").is_none());
}

#[test]
fn kh750_standby_enabled_is_display_only() {
    let (_simulator, _directory, harness) = speaker_with_panel(SimModel::Kh750, PanelTab::Standby);
    assert_eq!(role_of(&harness, STANDBY_LABEL), Role::Label);
    assert!(harness.query_all_by_role(Role::CheckBox).all(|node| node
        .accesskit_node()
        .label()
        .as_deref()
        != Some(STANDBY_LABEL)));
}

#[test]
fn kh120ii_standby_enabled_is_checkbox() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    assert_eq!(role_of(&harness, STANDBY_LABEL), Role::CheckBox);
    harness.get_by_label(STANDBY_LABEL).click();
    wait_until(&mut harness, "standby off on the device", |_| {
        simulator.value("device/standby/enabled") == Some(Value::Bool(false))
    });
}

#[test]
fn temperature_is_shown_in_celsius() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh750, PanelTab::Device);
    simulator.set_value("device/temperature", json!(313));
    wait_until(&mut harness, "40 °C", |harness| {
        harness.query_by_label("40 °C").is_some()
    });
}

/// The JSON pointers into the simulator's requests are the paths they name.
#[test]
fn the_test_pointers_are_their_paths() {
    let pointer = |path: &str| format!("/{path}");
    assert_eq!(MUTE_POINTER, pointer(paths::MUTE));
    assert_eq!(NAME_POINTER, pointer(paths::DEVICE_NAME));
    assert_eq!(DELAY_POINTER, pointer(paths::DELAY));
    assert_eq!(STANDBY_TIME_POINTER, pointer(paths::STANDBY_TIME));
    assert_eq!(LEVEL_POINTER, pointer(paths::OUTPUT_LEVEL));
}

#[test]
fn name_field_shows_refusal() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    click_name_field(&mut harness);
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Küche");
    harness.run_steps(1);
    harness.get_by_label(texts::APPLY.get()).click();
    harness.run_steps(2);
    barrier(&simulator);
    let sets = sets(&simulator, NAME_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
    let refusal = texts::describe_value_error(&ValueError::NotPrintableAscii);
    let refusal = harness.get_by_label(&refusal).rect();
    let field = harness.get_by_role(Role::TextInput);
    assert_eq!(field.value().as_deref(), Some("Küche"));
    assert!(
        refusal.min.y > field.rect().max.y,
        "the reason is below the field"
    );
    let next_row = harness.get_by_label(texts::SAVE_PRESET.get()).rect();
    assert!(
        refusal.max.y < next_row.min.y,
        "the reason is not the speaker's message"
    );
}

#[test]
fn control_mode_is_shown_read_only_on_both_models() {
    for model in [SimModel::Kh120Ii, SimModel::Kh750] {
        let (_simulator, _directory, harness) = speaker_with_panel(model, PanelTab::Device);
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
    open_panel_on(&mut harness, KH120_DEVICE_NAME, PanelTab::Standby);
    // The panel row is text; the strip's auto standby button is there but cannot be pressed.
    let standby: Vec<_> = harness.query_all_by_label(STANDBY_LABEL).collect();
    assert!(standby
        .iter()
        .any(|node| node.accesskit_node().role() == Role::Label));
    assert!(standby
        .iter()
        .filter(|node| node.accesskit_node().role() == Role::Button)
        .all(|node| node.accesskit_node().is_disabled()));
    assert!(harness.query_by_label(LOGO_LABEL).is_none());
}

/// The history shows the temperature; a model without one (KH 120 II) gets no history section.
#[test]
fn the_history_is_shown_only_for_a_model_with_a_temperature() {
    let title = texts::trends_title(neumann_control_app::trend::TREND_WINDOW_SECONDS);
    let (_monitor, _directory, monitor_panel) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Device);
    assert!(monitor_panel.query_by_label_contains(&title).is_none());
    let (_subwoofer, _directory, subwoofer_panel) =
        speaker_with_panel(SimModel::Kh750, PanelTab::Device);
    assert!(subwoofer_panel.query_by_label_contains(&title).is_some());
}

#[test]
fn number_field_sends_once_per_gesture() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    let start = number_field(&harness, DELAY_LABEL).rect().center();
    harness.drag_at(start);
    for frame in 1..=10 {
        harness.hover_at(start + egui::vec2(4.0 * frame as f32, 0.0));
        harness.step();
    }
    barrier(&simulator);
    assert!(
        sets(&simulator, DELAY_POINTER).is_empty(),
        "a Set went out while dragging"
    );
    harness.drop_at(start + egui::vec2(40.0, 0.0));
    wait_until(&mut harness, "the Set after release", |_| {
        !sets(&simulator, DELAY_POINTER).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert_eq!(sets.len(), 1, "{sets:?}");
    let delay = sets[0]
        .as_i64()
        .expect("the delay is sent as a whole number");
    assert!(delay > 0, "dragging right raises the delay: {sets:?}");
}

/// The panel's number field; the strip's knob for the same setting carries the same name.
fn number_field<'a>(harness: &'a Harness<'static, NeumannApp>, label: &'a str) -> Node<'a> {
    harness.get_by_role_and_label(Role::SpinButton, label)
}

/// Clicks a number field into text editing and types `text` over its value.
fn type_number(harness: &mut Harness<'static, NeumannApp>, label: &str, text: &str) {
    number_field(harness, label).click();
    harness.run_steps(1);
    number_field(harness, label).type_text(text);
    harness.run_steps(1);
}

fn numeric_value(harness: &Harness<'static, NeumannApp>, label: &str) -> Option<f64> {
    number_field(harness, label)
        .accesskit_node()
        .numeric_value()
}

#[test]
fn out_of_range_device_value_is_not_written_back() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(6000.0)
    });
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn visiting_a_number_field_does_not_write_its_rounded_text() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(100.123456));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(100.123456)
    });
    number_field(&harness, DELAY_LABEL).click();
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

/// Arrow Up in a focused field steps the value, as a screen reader's increment does; leaving
/// the field writes it.
#[test]
fn an_arrow_key_step_in_a_number_field_is_written() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    let before = numeric_value(&harness, STANDBY_TIME_LABEL).unwrap();
    number_field(&harness, STANDBY_TIME_LABEL).click();
    harness.run_steps(2);
    harness.key_press(egui::Key::ArrowUp);
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    wait_until(&mut harness, "the stepped value written", |_| {
        !sets(&simulator, STANDBY_TIME_POINTER).is_empty()
    });
    let sent = sets(&simulator, STANDBY_TIME_POINTER)[0].as_f64().unwrap();
    assert!(sent > before, "{sent} after {before}");
}

#[test]
fn visiting_an_out_of_range_number_field_writes_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(6000.0)
    });
    number_field(&harness, DELAY_LABEL).click();
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
    assert_eq!(numeric_value(&harness, DELAY_LABEL), Some(6000.0));
}

#[test]
fn a_number_typed_and_entered_in_one_frame_is_sent() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    number_field(&harness, STANDBY_TIME_LABEL).click();
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
    assert_eq!(sets(&simulator, STANDBY_TIME_POINTER), vec![json!(50)]);
}

#[test]
fn abandoned_number_edit_is_not_sent_on_reopen() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
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
    let sets = sets(&simulator, STANDBY_TIME_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn a_typed_number_is_snapped_only_when_sent() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    type_number(&mut harness, STANDBY_TIME_LABEL, "1.5");
    harness.run_steps(2);
    let shown = number_field(&harness, STANDBY_TIME_LABEL).value();
    assert_eq!(shown.as_deref(), Some("1.5"));
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the Set after Enter", |_| {
        !sets(&simulator, STANDBY_TIME_POINTER).is_empty()
    });
    assert_eq!(sets(&simulator, STANDBY_TIME_POINTER), vec![json!(2)]);
}

#[test]
fn escape_during_number_edit_sends_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, STANDBY_TIME_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
    assert_eq!(numeric_value(&harness, STANDBY_TIME_LABEL), Some(60.0));
}

#[test]
fn an_abandoned_name_draft_follows_the_device_again() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    click_name_field(&mut harness);
    harness.run_steps(1);
    harness.get_by_role(Role::TextInput).type_text("draft");
    harness.run_steps(1);
    let typed = harness
        .get_by_role(Role::TextInput)
        .value()
        .unwrap_or_default();
    assert!(typed.contains("draft"), "{typed}");
    // Somewhere else in the overview: the field loses its focus.
    harness.get_by_label(texts::STRIP_COLOR.get()).click();
    harness.run_steps(2);
    let shown = harness.get_by_role(Role::TextInput).value();
    assert_eq!(shown.as_deref(), Some("Simulated KH 120 II"));
}

#[test]
fn a_name_typed_then_tabbed_to_apply_is_sent() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    click_name_field(&mut harness);
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Studio");
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the Set after Enter on Übernehmen", |_| {
        !sets(&simulator, NAME_POINTER).is_empty()
    });
    barrier(&simulator);
    assert_eq!(sets(&simulator, NAME_POINTER), vec![json!("Studio")]);
}

#[test]
fn a_name_draft_left_by_tabbing_past_apply_follows_the_device_again() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    click_name_field(&mut harness);
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
fn the_fixed_part_of_the_strip_is_not_repeated_in_the_panel() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    // Mute and identify are listed in the panel too: a layout can take them off the strip.
    let mutes = harness.query_all_by_role_and_label(Role::CheckBox, texts::MUTE.get());
    assert_eq!(mutes.count(), 1);
    assert!(harness.query_by_label("Eingangspegel").is_none());
    let faders = harness.query_all_by_role_and_label(Role::Slider, texts::LEVEL.get());
    assert_eq!(faders.count(), 1);
    assert!(harness.query_all_by_role(Role::SpinButton).all(|node| node
        .accesskit_node()
        .label()
        .as_deref()
        != Some(texts::LEVEL.get())));
    show_tab(&mut harness, PanelTab::Device);
    assert!(harness.query_by_label("Identifizieren").is_some());
}

#[test]
fn kh750_output_meters_sit_next_to_the_input_meters() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh750);
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
fn kh120ii_strip_has_no_output_meters() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Device);
    scroll_to_and_click(&mut harness, texts::ALL_VALUES.get());
    wait_until(&mut harness, "the schema walk", |_| {
        simulator
            .requests()
            .iter()
            .any(|request| request.pointer("/osc/schema").is_some())
    });
}

/// Reloading a speaker that is gone would only swap the table for a failure: the window's
/// button follows the same rule as the panel's.
#[test]
fn all_values_cannot_be_reloaded_from_an_unreachable_speaker() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Device);
    scroll_to_and_click(&mut harness, texts::ALL_VALUES.get());
    let reload_enabled = |harness: &Harness<'static, NeumannApp>| {
        harness
            .query_by_label(texts::ALL_VALUES_RELOAD.get())
            .is_some_and(|button| !button.accesskit_node().is_disabled())
    };
    wait_until(&mut harness, "the values read", reload_enabled);
    simulator.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    assert!(!reload_enabled(&harness));
}

#[test]
fn panel_number_rows_keep_their_labels_on_one_line() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    // The panel row and the strip knob's caption carry the same name; neither may wrap.
    let rows = [
        (PanelTab::Standby, STANDBY_TIME_LABEL),
        (PanelTab::Device, LOGO_LABEL),
        (PanelTab::Outputs, DELAY_LABEL),
    ];
    for (tab, label) in rows {
        show_tab(&mut harness, tab);
        let rects: Vec<_> = harness
            .query_all_by_role_and_label(Role::Label, label)
            .map(|node| node.rect())
            .collect();
        assert!(!rects.is_empty(), "{label}");
        for rect in rects {
            assert!(rect.width() > rect.height(), "{label} wraps: {rect:?}");
        }
    }
    // The number field shows the value; the knob beside it does not repeat it, and the strip
    // shows a whole number as a whole number.
    // In the language's own decimal separator, or it could never be found.
    let decimal = format!("{} min", texts::one_decimal(60.0));
    assert!(harness.query_by_label_contains(&decimal).is_none());
}

#[test]
fn delay_is_typed_in_milliseconds_and_sent_in_samples() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    type_number(&mut harness, DELAY_LABEL, "10");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the delay Set", |_| {
        !sets(&simulator, DELAY_POINTER).is_empty()
    });
    assert_eq!(sets(&simulator, DELAY_POINTER), vec![json!(480)]);
}

#[test]
fn a_typed_number_outside_the_range_is_refused_not_clamped() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    // 500 ms are 24000 samples, far above the KH 120 II's 5760.
    type_number(&mut harness, DELAY_LABEL, "500");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
    // In the unit the delay is typed in, not in the speaker's samples.
    let range = Refusal::OutOfRange {
        min: 0.0,
        max: 120.0,
        unit: "ms".to_owned(),
    };
    harness.get_by_label(&texts::describe_refusal(&range));
}

#[test]
fn visiting_a_delay_in_milliseconds_writes_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    // One sample is 0.0208333… ms, which no decimal text holds exactly.
    simulator.set_value("audio/out/delay", json!(1));
    wait_until(&mut harness, "the reported delay", |harness| {
        harness.query_by_label_contains("0,02").is_some()
    });
    number_field(&harness, DELAY_LABEL).click();
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    assert!(sets(&simulator, DELAY_POINTER).is_empty());
}

#[test]
fn a_panel_drag_outside_the_range_writes_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(6000.0)
    });
    let start = number_field(&harness, DELAY_LABEL).rect().center();
    harness.drag_at(start);
    for frame in 1..=10 {
        harness.hover_at(start + egui::vec2(4.0 * frame as f32, 0.0));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(40.0, 0.0));
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn a_panel_drag_back_to_its_start_writes_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(100.123456));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(100.123456)
    });
    let start = number_field(&harness, DELAY_LABEL).rect().center();
    harness.drag_at(start);
    for offset in [8.0, 16.0, 8.0, 0.0] {
        harness.hover_at(start + egui::vec2(offset, 0.0));
        harness.step();
    }
    harness.drop_at(start);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, DELAY_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

#[test]
fn eq_rows_are_not_in_the_device_panel() {
    let (_simulator, _directory, harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    assert!(harness.query_by_label("Verstärkung").is_none());
    assert!(harness.query_by_label("Güte (Q)").is_none());
}

#[test]
fn the_panel_shows_the_temperature_trend_of_a_kh750() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh750, PanelTab::Device);
    simulator.set_value("device/temperature", json!(313));
    wait_until(&mut harness, "the trend at 40 °C", |harness| {
        harness
            .query_all_by_label_contains(&texts::temperature_trend("40 °C"))
            .next()
            .is_some()
    });
    harness.get_by_label(texts::WARNING_TREND.get());
}

/// The temperature line carries its lowest, highest and current value in °C.
#[test]
fn the_temperature_line_names_its_range() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh750, PanelTab::Device);
    // Below the simulator's 37 °C, so it is the lowest of the session.
    simulator.set_value("device/temperature", json!(300));
    wait_until(&mut harness, "27 °C", |harness| {
        harness
            .query_all_by_label_contains(&texts::temperature_trend("27 °C"))
            .next()
            .is_some()
    });
    simulator.set_value("device/temperature", json!(316));
    let range = texts::trend_range("27 °C", "43 °C");
    let line = format!("{} ({range})", texts::temperature_trend("43 °C"));
    wait_until(&mut harness, "the range at the line", |harness| {
        harness.query_all_by_label_contains(&line).next().is_some()
    });
}

#[test]
fn a_converted_delay_shows_only_the_converted_unit() {
    let (_simulator, _directory, harness) = speaker_with_panel(SimModel::Kh750, PanelTab::Outputs);
    let shown: Vec<String> = harness
        .query_all_by_role(Role::SpinButton)
        .filter_map(|field| field.accesskit_node().value().map(|text| text.to_string()))
        .collect();
    let delays: Vec<&String> = shown.iter().filter(|text| text.ends_with(" ms")).collect();
    assert!(!delays.is_empty(), "{shown:?}");
    assert!(
        shown.iter().all(|text| !text.contains("1/48 kHz")),
        "{shown:?}"
    );
}

const INPUT_MIX_POINTER: &str = "/audio/out/mixer/levels";

#[test]
fn the_input_mix_writes_the_whole_list() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    let before = simulator.value("audio/out/mixer/levels").unwrap();
    harness
        .query_all_by_role_and_label(Role::SpinButton, "Eingang B")
        .next()
        .expect("a field for input B")
        .click();
    harness.run_steps(1);
    harness
        .query_all_by_role_and_label(Role::SpinButton, "Eingang B")
        .next()
        .unwrap()
        .type_text("-20");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the mix written", |_| {
        !sets(&simulator, INPUT_MIX_POINTER).is_empty()
    });
    let mut expected = before.as_array().unwrap().clone();
    expected[1] = json!(-20.0);
    assert_eq!(
        sets(&simulator, INPUT_MIX_POINTER),
        vec![Value::Array(expected)]
    );
}

/// A speaker that reports the mix in whole numbers gets whole numbers back, as from a single
/// number field.
#[test]
fn the_input_mix_keeps_the_speakers_number_type() {
    let (simulator, _directory, mut harness) =
        connected_speaker_with(SimModel::Kh120Ii, |simulator| {
            simulator.set_value("audio/out/mixer/levels", json!([0, -6]));
        });
    open_panel_on(
        &mut harness,
        device_name(SimModel::Kh120Ii),
        PanelTab::Outputs,
    );
    harness
        .query_all_by_role_and_label(Role::SpinButton, "Eingang B")
        .next()
        .expect("a field for input B")
        .click();
    harness.run_steps(1);
    harness
        .query_all_by_role_and_label(Role::SpinButton, "Eingang B")
        .next()
        .unwrap()
        .type_text("-20");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the mix written", |_| {
        !sets(&simulator, INPUT_MIX_POINTER).is_empty()
    });
    assert_eq!(sets(&simulator, INPUT_MIX_POINTER), vec![json!([0, -20])]);
}

#[test]
fn mute_persist_is_a_checkbox_that_writes() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    assert_eq!(role_of(&harness, "Stumm-Persistenz"), Role::CheckBox);
    harness.get_by_label("Stumm-Persistenz").click();
    wait_until(&mut harness, "mute persist written", |_| {
        !sets(&simulator, "/audio/out/mute_persist").is_empty()
    });
    assert_eq!(
        sets(&simulator, "/audio/out/mute_persist"),
        vec![json!(true)]
    );
}

#[test]
fn kh750_shows_its_auto_standby_switch() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh750, PanelTab::Outputs);
    assert_eq!(
        harness
            .query_all_by_role_and_label(Role::SpinButton, "Eingang A")
            .count(),
        3
    );
    show_tab(&mut harness, PanelTab::Inputs);
    harness.get_by_label("Auto-Standby-Schalter");
}

#[test]
fn a_full_scan_writes_a_redacted_report_next_to_the_settings() {
    let (_simulator, directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Device);
    scroll_to_and_click(&mut harness, texts::FULL_SCAN.get());
    let written = |directory: &std::path::Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(directory)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("scan-"))
            })
            .collect()
    };
    wait_until(&mut harness, "both report files", |_| {
        written(directory.path()).len() == 2
    });
    for path in written(directory.path()) {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(name.starts_with("scan-KH-120-II-1_7_3-"), "{name}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("audio/out/level"), "{name}");
        assert!(!text.contains(SIMULATED_SERIAL), "{name} holds the serial");
        assert!(!text.contains(KH120_DEVICE_NAME), "{name} holds the name");
    }
    wait_until(&mut harness, "the report noted in the panel", |harness| {
        harness
            .query_by_label_contains(&texts::scan_written(""))
            .is_some()
    });
}

const LEVEL_POINTER: &str = "/audio/out/level";

#[test]
fn a_settings_preset_is_saved_and_written_back_as_one_entry() {
    let (simulator, directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    scroll_to_and_click(&mut harness, texts::SAVE_PRESET.get());
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, texts::PRESET_NAME.get())
        .click();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, texts::PRESET_NAME.get())
        .type_text("Evening");
    harness.run_steps(1);
    let delay = harness
        .query_all_by_role(Role::CheckBox)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Verzögerung"))
        })
        .expect("the delay row");
    delay.click();
    harness.run_steps(2);
    harness.get_by_label(texts::SAVE.get()).click();
    harness.run_steps(3);
    let saved = neumann_control_core::settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(saved.presets.len(), 1);
    let preset = &saved.presets[0];
    assert_eq!(preset.name, "Evening");
    assert!(preset.values.contains_key("audio/out/level"));
    assert!(!preset.values.contains_key("audio/out/delay"));
    assert!(!preset.values.contains_key("device/name"));

    simulator.set_value("audio/out/level", json!(70.0));
    wait_for_level(&mut harness, 70.0);
    let entries_before = harness.state().history().len();
    if harness.query_by_label(texts::LOAD_PRESET.get()).is_none() {
        open_panel(&mut harness, device_name(SimModel::Kh120Ii));
    }
    scroll_to_and_click(&mut harness, texts::LOAD_PRESET.get());
    harness.run_steps(3);
    harness
        .get_by_label(&texts::preset_entry("Evening", "KH 120 II"))
        .click();
    // The preview grows the dialog, which centres itself again over a few frames.
    harness.run_steps(6);
    // Side by side: what the speaker has now, what the preset writes.
    harness.get_by_label(texts::PRESET_NOW.get());
    harness.get_by_label(texts::PRESET_WRITTEN.get());
    // A label's text is its accessible value.
    let levels: Vec<String> = harness
        .query_all_by_role(Role::Label)
        .filter_map(|node| node.value())
        .filter(|text| text.ends_with("dB SPL"))
        .collect();
    assert!(
        levels.iter().any(|level| level.starts_with("70")),
        "{levels:?}"
    );
    assert!(
        levels.iter().any(|level| level.starts_with("100")),
        "{levels:?}"
    );
    harness.get_by_label(texts::WRITE_PRESET.get()).click();
    wait_until(&mut harness, "the level written back", |_| {
        sets(&simulator, LEVEL_POINTER).contains(&json!(100.0))
    });
    harness.run_steps(2);
    assert_eq!(harness.state().history().len(), entries_before + 1);
}

/// A stored preset can hold a value this speaker refuses (another model's range, a file edited
/// by hand). Loading it then writes nothing, not the part before the refused value.
#[test]
fn a_settings_preset_with_a_refused_value_writes_nothing() {
    let far = SettingsPreset {
        name: "Far".to_owned(),
        product: "KH 120 II".to_owned(),
        values: [
            ("audio/out/level".to_owned(), json!(90.0)),
            ("audio/out/delay".to_owned(), json!(99_999)),
        ]
        .into(),
    };
    let (simulator, _directory, mut harness) =
        connected_speaker_with_presets(SimModel::Kh120Ii, vec![far]);
    open_stored_preset(
        &mut harness,
        device_name(SimModel::Kh120Ii),
        "Far",
        "KH 120 II",
    );
    let range = Refusal::OutOfRange {
        min: 0.0,
        max: 120.0,
        unit: "ms".to_owned(),
    };
    harness.get_by_label(&texts::setting_refusal(
        DELAY_LABEL,
        &texts::describe_refusal(&range),
    ));
    harness.get_by_label(texts::WRITE_PRESET.get()).click();
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    assert!(sets(&simulator, LEVEL_POINTER).is_empty());
    assert!(sets(&simulator, DELAY_POINTER).is_empty());
}

/// What is in the field when it is left counts, not a number typed on the way there: a field
/// emptied or made unreadable writes nothing.
#[test]
fn a_number_field_left_empty_or_unreadable_writes_nothing() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Standby);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50");
    harness.key_press(egui::Key::Backspace);
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(1);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    type_number(&mut harness, STANDBY_TIME_LABEL, "50x");
    harness.key_press(egui::Key::Tab);
    harness.run_steps(IDLE_FRAMES);
    barrier(&simulator);
    let sets = sets(&simulator, STANDBY_TIME_POINTER);
    assert!(sets.is_empty(), "{sets:?}");
}

/// A second click before the speaker answered the first goes from the value just sent, as on
/// the strip: mute and unmute, not mute twice.
#[test]
fn a_second_switch_click_before_the_answer_undoes_the_first() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    // Under the client's answer timeout, so the first write stays unconfirmed without a disconnect.
    simulator.set_reply_delay(std::time::Duration::from_secs(2));
    let mute = texts::MUTE.get();
    harness.get_by_role_and_label(Role::CheckBox, mute).click();
    harness.run_steps(1);
    harness.get_by_role_and_label(Role::CheckBox, mute).click();
    // Each request waits for its delayed answer, so the second write comes seconds later.
    wait_until(&mut harness, "both mute writes", |_| {
        sets(&simulator, MUTE_POINTER).len() >= 2
    });
    assert_eq!(
        sets(&simulator, MUTE_POINTER),
        vec![json!(true), json!(false)]
    );
}

#[test]
fn deleting_a_settings_preset_asks_first() {
    let (_simulator, directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    scroll_to_and_click(&mut harness, texts::SAVE_PRESET.get());
    harness.run_steps(3);
    let field = || texts::PRESET_NAME.get();
    harness
        .get_by_role_and_label(Role::TextInput, field())
        .click();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, field())
        .type_text("Evening");
    harness.run_steps(1);
    harness.get_by_label(texts::SAVE.get()).click();
    harness.run_steps(3);
    let presets =
        || neumann_control_core::settings::load(&directory.path().join(SETTINGS_FILE)).presets;
    if harness.query_by_label(texts::LOAD_PRESET.get()).is_none() {
        open_panel(&mut harness, device_name(SimModel::Kh120Ii));
    }
    scroll_to_and_click(&mut harness, texts::LOAD_PRESET.get());
    harness.run_steps(3);
    harness
        .get_by_label(&texts::preset_entry("Evening", "KH 120 II"))
        .click();
    harness.run_steps(6);
    harness.get_by_label(texts::DELETE_PRESET.get()).click();
    harness.run_steps(3);
    harness.get_by_label(&texts::delete_question("Evening"));
    assert_eq!(presets().len(), 1);
    harness.get_by_label(texts::DELETE_PRESET.get()).click();
    harness.run_steps(3);
    assert!(presets().is_empty());
}

/// Escape in the filter ends the typing; only the next one closes the window.
#[test]
fn escape_in_the_all_values_filter_keeps_the_window() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Device);
    scroll_to_and_click(&mut harness, texts::ALL_VALUES.get());
    let filter = || texts::ALL_VALUES_FILTER_LABEL.get();
    wait_until(&mut harness, "the filter", |harness| {
        harness
            .query_by_role_and_label(Role::TextInput, filter())
            .is_some()
    });
    harness
        .get_by_role_and_label(Role::TextInput, filter())
        .click();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, filter())
        .type_text("level");
    harness.run_steps(1);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::TextInput, filter());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::TextInput, filter())
        .is_none());
}

/// Escape with a list open closes the list, not the panel.
#[test]
fn escape_on_an_open_list_keeps_the_panel() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    harness
        .query_all_by_role(Role::ComboBox)
        .next()
        .expect("a choice in the panel")
        .click();
    harness.run_steps(2);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.get_by_label(texts::TAB_OVERVIEW.get());
}

/// The history shows a delay as the panel does, not in the speaker's samples.
#[test]
fn the_history_shows_a_delay_in_its_shown_unit() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    type_number(&mut harness, DELAY_LABEL, "10");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the delay write", |_| {
        !sets(&simulator, DELAY_POINTER).is_empty()
    });
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.get_by_label(texts::HISTORY.get()).click();
    harness.run_steps(3);
    let line = harness
        .query_all_by_label_contains(" → ")
        .find_map(|node| {
            let node = node.accesskit_node();
            let text = node.label().or(node.value())?;
            text.contains(DELAY_LABEL).then_some(text)
        })
        .expect("the delay's history line");
    assert!(line.ends_with("10,00 ms"), "{line}");
}

/// Another tab stays while the panel is open; opened again, it starts on the overview.
#[test]
fn the_panel_opens_on_the_overview_again() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    harness.get_by_label(texts::REMOVE.get());
    show_tab(&mut harness, PanelTab::Outputs);
    assert!(harness.query_by_label(texts::REMOVE.get()).is_none());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::TAB_OVERVIEW.get()).is_none());
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.get_by_label(texts::REMOVE.get());
}

/// The name is set on the overview only; the device tab shows it as text.
#[test]
fn the_name_is_edited_on_the_overview_and_shown_on_the_device_tab() {
    let (_simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Overview);
    harness.get_by_role(Role::TextInput);
    show_tab(&mut harness, PanelTab::Device);
    assert!(harness.query_by_role(Role::TextInput).is_none());
    assert!(harness.query_by_label(texts::APPLY.get()).is_none());
    assert_eq!(role_of(&harness, "Name"), Role::Label);
}

/// 184 samples are 3.8333… ms: the field shows two decimals, not the binary fraction.
#[test]
fn a_delay_field_shows_its_milliseconds_rounded() {
    let (simulator, _directory, mut harness) =
        speaker_with_panel(SimModel::Kh120Ii, PanelTab::Outputs);
    simulator.set_value("audio/out/delay", json!(184));
    wait_until(&mut harness, "the reported delay", |harness| {
        numeric_value(harness, DELAY_LABEL) == Some(184.0)
    });
    let shown = number_field(&harness, DELAY_LABEL).value();
    assert_eq!(shown.as_deref(), Some("3,83 ms"));
}
