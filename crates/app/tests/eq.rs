//! The EQ window: one per speaker, a graph of every container and a table of the chosen one.
//! A band edit writes that band only, at the end of the gesture, and lands in the history.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_app::writes::Refusal;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::{json, Value};
use support::*;

const GAIN_LABEL: &str = "Verstärkung";
const GAIN_POINTER: &str = "/audio/out/eq2/gain";
const FREQUENCY_POINTER: &str = "/audio/out/eq2/frequency";

/// Ten bands spread over the range, so every point has its own place.
fn spread(simulator: &Simulator) {
    let frequencies: Vec<f64> = (0..10).map(|band| 50.0 * 2f64.powi(band)).collect();
    simulator.set_value("audio/out/eq2/frequency", json!(frequencies));
}

fn open_eq(model: SimModel) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_speaker_with(model, spread);
    open_panel(&mut harness, device_name(model));
    harness.get_by_label(texts::EQ.get()).click();
    wait_for_label(&mut harness, &texts::eq_band(1));
    (simulator, directory, harness)
}

fn drag(harness: &mut Harness<'static, NeumannApp>, from: egui::Pos2, by: egui::Vec2) {
    harness.drag_at(from);
    for frame in 1..=5 {
        harness.hover_at(from + by * (frame as f32 / 5.0));
        harness.step();
    }
    harness.drop_at(from + by);
    harness.run_steps(3);
}

fn gain_fields(harness: &Harness<'static, NeumannApp>) -> Vec<bool> {
    harness
        .query_all_by_role_and_label(Role::SpinButton, GAIN_LABEL)
        .map(|field| field.accesskit_node().is_disabled())
        .collect()
}

#[test]
fn the_eq_button_opens_the_window_with_both_containers() {
    let (_simulator, _directory, harness) = open_eq(SimModel::Kh120Ii);
    harness.get_by_label("EQ 2");
    harness.get_by_label("EQ 3");
    assert_eq!(gain_fields(&harness), vec![false; 10]);
}

#[test]
fn dragging_a_point_writes_frequency_and_gain_of_that_band_only() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let point = harness.get_by_label(&texts::eq_band(3)).rect().center();
    drag(&mut harness, point, egui::vec2(30.0, -40.0));
    wait_until(&mut harness, "the band writes", |_| {
        !sets(&simulator, GAIN_POINTER).is_empty()
            && !sets(&simulator, FREQUENCY_POINTER).is_empty()
    });
    let gain = sets(&simulator, GAIN_POINTER)[0].clone();
    let entries = gain.as_array().unwrap();
    assert_eq!(entries.len(), 10);
    let only_band_3 = entries
        .iter()
        .enumerate()
        .all(|(band, entry)| (band == 2) != entry.is_null());
    assert!(only_band_3, "{gain}");
    assert!(entries[2].as_f64().unwrap() > 0.0);
    let frequency = sets(&simulator, FREQUENCY_POINTER)[0][2].as_f64().unwrap();
    assert!(frequency > 200.0, "{frequency}");
    let history: Vec<_> = harness.state().history().entries().cloned().collect();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].changes.len(), 2);
}

#[test]
fn a_drag_beyond_the_edge_stays_in_the_limits() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let point = harness.get_by_label(&texts::eq_band(10)).rect().center();
    drag(&mut harness, point, egui::vec2(2000.0, -2000.0));
    wait_until(&mut harness, "the band writes", |_| {
        !sets(&simulator, FREQUENCY_POINTER).is_empty()
            && !sets(&simulator, GAIN_POINTER).is_empty()
    });
    let frequency = sets(&simulator, FREQUENCY_POINTER)[0][9].as_f64().unwrap();
    assert!(frequency <= 24000.0, "{frequency}");
    let gain = sets(&simulator, GAIN_POINTER)[0][9].as_f64().unwrap();
    assert!(gain <= 24.0, "{gain}");
}

#[test]
fn a_typed_gain_out_of_range_is_refused() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    harness
        .query_all_by_role_and_label(Role::SpinButton, GAIN_LABEL)
        .next()
        .unwrap()
        .click();
    harness.run_steps(1);
    harness
        .query_all_by_role_and_label(Role::SpinButton, GAIN_LABEL)
        .next()
        .unwrap()
        .type_text("30");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    barrier(&simulator);
    assert!(sets(&simulator, GAIN_POINTER).is_empty());
    let range = ValueError::OutOfRange {
        min: -99.0,
        max: 24.0,
    };
    harness.get_by_label_contains(&texts::describe_refusal(&Refusal::Invalid(range)));
}

#[test]
fn undo_of_a_band_edit_writes_that_band_back() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let point = harness.get_by_label(&texts::eq_band(2)).rect().center();
    drag(&mut harness, point, egui::vec2(0.0, -40.0));
    wait_until(&mut harness, "the gain write", |_| {
        !sets(&simulator, GAIN_POINTER).is_empty()
    });
    let id = harness.state().history().newest_open().unwrap();
    harness.state_mut().request_take_back(id);
    wait_until(&mut harness, "the gain written back", |_| {
        sets(&simulator, GAIN_POINTER).len() == 2
    });
    let mut expected = vec![Value::Null; 10];
    expected[1] = json!(0.0);
    assert_eq!(sets(&simulator, GAIN_POINTER)[1], Value::Array(expected));
}

#[test]
fn a_read_only_container_has_no_editable_fields() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    harness.get_by_label("EQ 3").click();
    harness.run_steps(2);
    assert_eq!(gain_fields(&harness), vec![true; 20]);
}

#[test]
fn the_crossover_is_shown_on_the_kh750() {
    let (_simulator, _directory, harness) = open_eq(SimModel::Kh750);
    harness.get_by_label("Weiche 1");
    harness.get_by_label(texts::EQ_OUTPUT_ALL.get());
}

#[test]
fn an_unreachable_speaker_greys_the_window() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    simulator.stop();
    wait_until(&mut harness, "the fields greyed", |harness| {
        gain_fields(harness).iter().all(|disabled| *disabled)
    });
}

#[test]
fn a_band_change_reads_as_one_band_in_the_history() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let point = harness.get_by_label(&texts::eq_band(2)).rect().center();
    drag(&mut harness, point, egui::vec2(0.0, -40.0));
    wait_until(&mut harness, "the gain write", |_| {
        !sets(&simulator, GAIN_POINTER).is_empty()
    });
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::HISTORY.get()).click();
    harness.run_steps(2);
    harness.get_by_label_contains(&texts::eq_change_setting("EQ 2", 2, GAIN_LABEL));
}
