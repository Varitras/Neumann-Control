//! The speaker card in the real window: values, fader, buttons, unreachable state.

mod support;

use eframe::egui::{self, accesskit::Role, MouseWheelUnit, TouchPhase};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::paths;
use neumann_control_app::state::{SpeakerKey, SpeakerState};
use neumann_control_app::texts;
use neumann_control_app::views::all_values;
use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::Event;
use serde_json::Value;
use support::*;

#[test]
fn card_shows_level_and_name() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label("KH 120 II");
    harness.get_by_label("Simulated KH 120 II");
    harness.get_by_label(&readout(100.0));
    harness.get_by_label_contains(&mask_serial(SIMULATED_SERIAL));
    assert!(harness.query_by_label_contains(SIMULATED_SERIAL).is_none());
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn fader_drag_sends_once_on_release() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let rect = harness.get_by_role(Role::Slider).rect();
    let start = rect.center();
    harness.drag_at(start);
    for frame in 1..=10 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "a Set went out while dragging"
    );
    let shown = harness
        .get_by_role(Role::Slider)
        .accesskit_node()
        .numeric_value()
        .unwrap();
    harness.drop_at(start + egui::vec2(0.0, 40.0));
    wait_until(&mut harness, "the Set after release", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = level_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0] < 100.0, "dragging down lowers the level: {sets:?}");
    assert_eq!(shown, sets[0]);
}

#[test]
fn fader_value_is_shown_at_the_handle_while_dragging() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let start = harness.get_by_role(Role::Slider).rect().center();
    harness.drag_at(start);
    for frame in 1..=5 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    let dragged = harness
        .get_by_role(Role::Slider)
        .accesskit_node()
        .numeric_value()
        .unwrap();
    assert!(dragged < 100.0);
    // Once under the fader, once at its handle.
    assert_eq!(harness.query_all_by_label(&readout(dragged)).count(), 2);
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    harness.run_steps(2);
    barrier(&simulator);
    assert_eq!(harness.query_all_by_label(&readout(dragged)).count(), 1);
}

#[test]
fn wheel_over_fader_sends_one_set_after_the_wheel_went_quiet() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let center = harness.get_by_role(Role::Slider).rect().center();
    harness.hover_at(center);
    for _ in 0..2 {
        harness.event(egui::Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
    barrier(&simulator);
    assert!(level_sets(&simulator).is_empty(), "the wheel wrote at once");
    harness.get_by_label(&readout(101.0));
    wait_until(&mut harness, "the wheel Set", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    assert_eq!(level_sets(&simulator), vec![101.0]);
}

#[test]
fn escape_during_a_drag_sends_nothing() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let start = harness.get_by_role(Role::Slider).rect().center();
    harness.drag_at(start);
    for frame in 1..=5 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    harness.key_press(egui::Key::Escape);
    harness.step();
    for frame in 6..=10 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 40.0));
    harness.run_steps(10);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
    harness.get_by_label(&readout(100.0));
    // The next drag is a normal one again.
    harness.drag_at(start);
    harness.hover_at(start + egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "the next drag's Set", |_| {
        !level_sets(&simulator).is_empty()
    });
}

#[test]
fn a_drag_replaces_wheel_steps_not_yet_sent() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let start = harness.get_by_role(Role::Slider).rect().center();
    harness.hover_at(start);
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "the drag's Set", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = level_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0] < 100.0, "{sets:?}");
}

#[test]
fn mute_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::MUTE).click();
    wait_until(&mut harness, "mute on the device", |_| {
        simulator.value("audio/out/mute") == Some(Value::Bool(true))
    });
}

#[test]
fn identify_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::IDENTIFY).click();
    wait_until(&mut harness, "identification on the device", |_| {
        simulator.value("device/identification/visual") == Some(Value::Bool(true))
    });
}

#[test]
fn more_toggles_to_less_and_back() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(2);
    harness.get_by_label(texts::LESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::MORE);
}

#[test]
fn unreachable_card_is_disabled() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    simulator.stop();
    wait_until(&mut harness, "the unreachable text", |harness| {
        harness.query_by_label(texts::UNREACHABLE).is_some()
    });
    assert!(harness
        .get_by_label(texts::MUTE)
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_label(texts::IDENTIFY)
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_role(Role::Slider)
        .accesskit_node()
        .is_disabled());
}

#[test]
fn out_of_range_level_is_not_written_back() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    simulator.set_value("audio/out/level", serde_json::json!(130.0));
    wait_for_level(&mut harness, 130.0);
    harness.run_steps(10);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
}

const KH120II_LEAF_COUNT: usize = 50;
const STANDBY_LEAF_COUNT: usize = 4;
const SPEAKER_NAME: &str = "Simulated KH 120 II";
const STANDBY_ENABLED_PATH: &str = "device/standby/enabled";
/// A refusal text as the worker produces it from a lost connection, address included.
const RAW_FAILURE: &str = "connection error: could not read from fe80::2a36:38ff:fe12:3456%7";

fn all_values_window(
    model: SimModel,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_card(model);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.get_by_label(texts::MORE).click();
    wait_until(&mut harness, "the expanded area", |harness| {
        harness.query_by_label(texts::ALL_VALUES).is_some()
    });
    harness.run_steps(1);
    harness.get_by_label(texts::ALL_VALUES).click();
    wait_until(&mut harness, "the loaded table", |harness| {
        harness
            .query_by_label(texts::ALL_VALUES_PATH_COLUMN)
            .is_some()
    });
    (simulator, directory, harness)
}

fn filter_all_values(harness: &mut Harness<'static, NeumannApp>, text: &str) {
    harness.get_by_label(texts::ALL_VALUES_FILTER_LABEL).click();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label(texts::ALL_VALUES_FILTER_LABEL)
        .type_text(text);
    harness.run_steps(3);
}

#[test]
fn all_values_lists_50_leaves_for_kh120ii() {
    let (_simulator, _directory, harness) = all_values_window(SimModel::Kh120Ii);
    harness.get_by_label(&texts::all_values_title(SPEAKER_NAME));
    harness.get_by_label(&texts::all_values_count(
        KH120II_LEAF_COUNT,
        KH120II_LEAF_COUNT,
    ));
}

#[test]
fn filter_narrows_rows() {
    let (_simulator, _directory, mut harness) = all_values_window(SimModel::Kh120Ii);
    filter_all_values(&mut harness, "standby");
    harness.get_by_label(&texts::all_values_count(
        STANDBY_LEAF_COUNT,
        KH120II_LEAF_COUNT,
    ));
    harness.get_by_label(STANDBY_ENABLED_PATH);
    assert!(harness.query_by_label("audio/out/level").is_none());
}

#[test]
fn known_column_marks_catalog_paths() {
    let (_simulator, _directory, mut harness) = all_values_window(SimModel::Kh120Ii);
    filter_all_values(&mut harness, STANDBY_ENABLED_PATH);
    harness.get_by_label(texts::ALL_VALUES_KNOWN);
    filter_all_values(&mut harness, "restore");
    harness.get_by_label("device/restore");
    assert!(harness.query_by_label(texts::ALL_VALUES_KNOWN).is_none());
}

#[test]
fn serial_is_masked_in_the_table() {
    let (_simulator, _directory, mut harness) = all_values_window(SimModel::Kh120Ii);
    filter_all_values(&mut harness, paths::SERIAL);
    harness.get_by_label(paths::SERIAL);
    harness.get_by_label(&mask_serial(SIMULATED_SERIAL));
    assert!(harness.query_by_label_contains(SIMULATED_SERIAL).is_none());
}

#[test]
fn failure_is_shown() {
    let address = parse_address("fe80::2a36:38ff:fe12:3456%7", 45).unwrap();
    let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
    speaker.read_all_values();
    speaker.apply(Event::AllValuesFailed(RAW_FAILURE.to_owned()), 0.0);
    let catalog = Catalog::embedded();
    let harness = Harness::new_ui(|ui| {
        all_values::show(ui.ctx(), &mut speaker, catalog);
    });
    harness.get_by_label(texts::ALL_VALUES_FAILED);
    assert!(harness.query_by_label_contains("fe12:3456").is_none());
    assert!(harness
        .query_by_label_contains("connection error")
        .is_none());
}

#[test]
fn card_content_stacks_top_down() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let second = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(
        directory.path(),
        vec![manual_for(&first), manual_for(&second)],
    );
    let mut harness = harness(path, Vec::new());
    wait_until(&mut harness, "both cards connected", |harness| {
        harness.query_all_by_role(Role::Slider).count() == 2
            && harness.query_by_label("KH 120 II").is_some()
            && harness.query_by_label("KH 750").is_some()
    });
    harness.run_steps(3);
    let names = [
        harness.get_by_label("KH 120 II").rect(),
        harness.get_by_label("KH 750").rect(),
    ];
    let mutes: Vec<_> = harness
        .query_all_by_label(texts::MUTE)
        .map(|node| node.rect())
        .collect();
    let mores: Vec<_> = harness
        .query_all_by_label(texts::MORE)
        .map(|node| node.rect())
        .collect();
    assert_eq!(mutes.len(), 2);
    assert_eq!(mores.len(), 2);
    for (index, name) in names.iter().enumerate() {
        let card = if mutes[0].center().x < mutes[1].center().x {
            index
        } else {
            1 - index
        };
        assert!(name.bottom() <= mutes[card].top(), "name above Stumm");
        assert!(
            mutes[card].bottom() <= mores[card].top(),
            "Mehr below Stumm"
        );
    }
    assert!(
        (names[0].top() - names[1].top()).abs() < 1.0,
        "cards start at the same height: {names:?}"
    );
}
