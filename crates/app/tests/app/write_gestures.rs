//! A write is the end of a deliberate gesture on that control and carries the value the gesture
//! chose, never a value rounded or clamped behind the user's back. These tests hold that line
//! for each kind of control: strip fader, group fader, typed entry and panel knob.

use crate::support::*;
use eframe::egui::{self, accesskit::Role, MouseWheelUnit, TouchPhase};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_app::writes::Refusal;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::simulator::SimModel;
use serde_json::{json, Value};

/// The strip's delay knob carries the device panel's name.
const DELAY_KNOB: &str = "Verzögerung";

const LEVEL_PATH: &str = "audio/out/level";

fn wheel(harness: &mut Harness<'static, NeumannApp>, notches: f32) {
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, notches),
        phase: TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
}

fn slider_center(harness: &Harness<'static, NeumannApp>, label: &str) -> egui::Pos2 {
    harness
        .get_by_role_and_label(Role::Slider, label)
        .rect()
        .center()
}

fn click_at(harness: &mut Harness<'static, NeumannApp>, position: egui::Pos2) {
    harness.drag_at(position);
    harness.step();
    harness.drop_at(position);
    harness.run_steps(5);
}

fn levels(simulator: &neumann_control_core::simulator::Simulator) -> Vec<Value> {
    sets(simulator, "/audio/out/level")
}

#[test]
fn a_click_on_a_fader_without_moving_writes_nothing() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value(LEVEL_PATH, json!(90.3));
    wait_for_level(&mut harness, 90.3);
    let center = slider_center(&harness, texts::LEVEL.get());
    click_at(&mut harness, center);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
}

#[test]
fn a_click_on_a_group_fader_without_moving_writes_nothing() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    // The KH 120 II is the loudest member, so its off-grid level is the group's reference.
    kh750.set_value(LEVEL_PATH, json!(80.0));
    kh120.set_value(LEVEL_PATH, json!(90.3));
    wait_for_level(&mut harness, 80.0);
    wait_until(&mut harness, "the off-grid level", |harness| {
        harness.query_by_label(&readout(90.3)).is_some()
    });
    let center = slider_center(&harness, &texts::group_level(GROUP_NAME));
    click_at(&mut harness, center);
    barrier(&kh120);
    barrier(&kh750);
    assert!(levels(&kh120).is_empty() && levels(&kh750).is_empty());
}

#[test]
fn a_typed_level_outside_the_range_is_refused_not_clamped() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let position = harness.get_by_label(&readout(100.0)).rect().center();
    double_click_at(&mut harness, position);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("1000");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
    let range = ValueError::OutOfRange {
        min: 0.0,
        max: 120.0,
    };
    harness.get_by_label(&texts::describe_refusal(&Refusal::Invalid(range)));
}

#[test]
fn the_wheel_on_a_panel_knob_does_not_scroll_another_knob_under_the_pointer() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    // Short enough that the panel scrolls (its delay knob stays below the window), tall enough
    // that the logo knob is in view; `open_panel` makes the window tall.
    harness.set_size(egui::vec2(1200.0, 480.0));
    harness.run_steps(2);
    let label = texts::knob_label("Logo-Helligkeit");
    let center = slider_center(&harness, &label);
    harness.hover_at(center);
    for _ in 0..8 {
        wheel(&mut harness, -1.0);
        harness.step();
    }
    wait_until(&mut harness, "the logo write", |_| {
        !sets(&simulator, "/ui/logo/brightness").is_empty()
    });
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        delay_sets(&simulator).is_empty(),
        "{:?}",
        delay_sets(&simulator)
    );
}

#[test]
fn a_group_move_is_relative_to_where_the_drag_started() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    kh750.set_value(LEVEL_PATH, json!(90.0));
    wait_for_level(&mut harness, 90.0);
    let start = slider_center(&harness, &texts::group_level(GROUP_NAME));
    // A short move down (about 3 dB), smaller than the change of the loudest member below.
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, 1.25 * frame as f32));
        harness.step();
    }
    // The loudest member changes while the group fader is held.
    kh120.set_value(LEVEL_PATH, json!(80.0));
    wait_for_level(&mut harness, 80.0);
    harness.drop_at(start + egui::vec2(0.0, 5.0));
    wait_until(&mut harness, "both level sets", |_| {
        !levels(&kh120).is_empty() && !levels(&kh750).is_empty()
    });
    let first = levels(&kh120)[0].as_f64().unwrap();
    let second = levels(&kh750)[0].as_f64().unwrap();
    assert!(second < 90.0, "the KH 750 went up: {second}");
    assert!(
        ((first - 80.0) - (second - 90.0)).abs() < 1e-9,
        "{first} {second}"
    );
}

#[test]
fn a_group_raises_its_quieter_members_when_the_loudest_is_at_its_limit() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    kh120.set_value(LEVEL_PATH, json!(120.0));
    kh750.set_value(LEVEL_PATH, json!(90.0));
    wait_for_level(&mut harness, 120.0);
    wait_for_level(&mut harness, 90.0);
    let start = slider_center(&harness, &texts::group_level(GROUP_NAME));
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start - egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start - egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "the KH 750 set", |_| {
        !levels(&kh750).is_empty()
    });
    harness.run_steps(5);
    barrier(&kh120);
    assert!(levels(&kh750)[0].as_f64().unwrap() > 90.0);
    assert!(levels(&kh120).is_empty(), "{:?}", levels(&kh120));
}

#[test]
fn scrolling_the_panel_from_a_gap_never_turns_a_knob() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.set_size(egui::vec2(1200.0, 420.0));
    harness.run_steps(2);
    let knob = slider_center(&harness, &texts::knob_label("Logo-Helligkeit"));
    // Above the knob, on no control: the wheel scrolls the panel, and knobs pass under it.
    harness.hover_at(knob - egui::vec2(0.0, 40.0));
    for _ in 0..12 {
        wheel(&mut harness, -1.0);
        harness.step();
    }
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        delay_sets(&simulator).is_empty(),
        "{:?}",
        delay_sets(&simulator)
    );
    assert!(sets(&simulator, "/ui/logo/brightness").is_empty());
}

#[test]
fn a_knob_on_an_out_of_range_value_turned_further_out_writes_nothing() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value("audio/out/delay", json!(6000));
    wait_until(&mut harness, "the reported delay", |harness| {
        harness.query_by_label_contains("125,00 ms").is_some()
    });
    let end = turn_knob(&mut harness, DELAY_KNOB, 90.0, 150.0);
    harness.drop_at(end);
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        delay_sets(&simulator).is_empty(),
        "{:?}",
        delay_sets(&simulator)
    );
}

#[test]
fn a_fader_on_an_out_of_range_level_pushed_further_out_writes_nothing() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value(LEVEL_PATH, json!(125.0));
    wait_for_level(&mut harness, 125.0);
    let start = slider_center(&harness, texts::LEVEL.get());
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start - egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start - egui::vec2(0.0, 20.0));
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
}

#[test]
fn a_typed_delay_outside_the_range_is_refused_on_the_strip_knob() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let position = slider_center(&harness, DELAY_KNOB);
    double_click_at(&mut harness, position);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("5000");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        delay_sets(&simulator).is_empty(),
        "{:?}",
        delay_sets(&simulator)
    );
}

#[test]
fn a_typed_level_just_outside_the_range_is_refused_too() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let position = harness.get_by_label(&readout(100.0)).rect().center();
    double_click_at(&mut harness, position);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("120,2");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(5);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
}
