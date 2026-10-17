//! The speaker card in the real window: values, fader, buttons, unreachable state.

mod support;

use eframe::egui::{self, accesskit::Role, MouseWheelUnit, TouchPhase};
use egui_kittest::kittest::{NodeT, Queryable};
use neumann_control_app::texts;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::simulator::SimModel;
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
