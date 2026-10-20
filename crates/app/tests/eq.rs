//! The EQ window: one per speaker, a graph of every container and a table of the chosen one.
//! A band edit writes that band only, at the end of the gesture, and lands in the history.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::eq_file;
use neumann_control_app::texts;
use neumann_control_app::writes::Refusal;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::settings::CrossoverGuard;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::{json, Value};
use support::*;

const GAIN_LABEL: &str = "Pegel";
const BOOST_LABEL: &str = "Anhebung";
const GAIN_POINTER: &str = "/audio/out/eq2/gain";
const BOOST_POINTER: &str = "/audio/out/eq2/boost";
const FREQUENCY_POINTER: &str = "/audio/out/eq2/frequency";

/// Ten bands spread over the range, so every point has its own place.
fn spread(simulator: &Simulator) {
    let frequencies: Vec<f64> = (0..10).map(|band| 50.0 * 2f64.powi(band)).collect();
    simulator.set_value("audio/out/eq2/frequency", json!(frequencies));
    // Only bands that are on have a point to drag.
    simulator.set_value("audio/out/eq2/enabled", json!(vec![true; 10]));
}

/// Opened from the strip: the KH 750's panel is taller than the test window, so its EQ button
/// lies outside it.
fn open_eq(model: SimModel) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    open_eq_with(model, |_| {})
}

/// Like `open_eq`, with values set on the simulator (after the spread) before the first read.
fn open_eq_with(
    model: SimModel,
    prepare: impl FnOnce(&Simulator),
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_speaker_with(model, |simulator| {
        spread(simulator);
        prepare(simulator);
    });
    // The embedded window is cut off in the small window, its close button with it.
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    (simulator, directory, harness)
}

/// The strip has an "EQ" button too; the panel's lies below it, at the panel's foot.
fn click_panel_eq(harness: &mut Harness<'static, NeumannApp>) {
    let lowest = harness
        .query_all_by_role_and_label(Role::Button, texts::EQ.get())
        .max_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
        .expect("an EQ button");
    lowest.click();
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
fn the_panels_eq_button_opens_the_window_with_both_containers() {
    let (_simulator, _directory, mut harness) = connected_speaker_with(SimModel::Kh120Ii, spread);
    open_panel(&mut harness, device_name(SimModel::Kh120Ii));
    click_panel_eq(&mut harness);
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    harness.get_by_label("EQ 2");
    harness.get_by_label("EQ 3");
    assert_eq!(gain_fields(&harness), vec![false; 10]);
}

#[test]
fn dragging_a_point_writes_frequency_and_boost_of_that_band_only() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let point = harness.get_by_label(&texts::eq_band(3)).rect().center();
    drag(&mut harness, point, egui::vec2(30.0, -40.0));
    wait_until(&mut harness, "the band writes", |_| {
        !sets(&simulator, BOOST_POINTER).is_empty()
            && !sets(&simulator, FREQUENCY_POINTER).is_empty()
    });
    // The flat gain stays: dragging a point shapes the filter, not the level of everything.
    assert!(sets(&simulator, GAIN_POINTER).is_empty());
    let gain = sets(&simulator, BOOST_POINTER)[0].clone();
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
            && !sets(&simulator, BOOST_POINTER).is_empty()
    });
    let frequency = sets(&simulator, FREQUENCY_POINTER)[0][9].as_f64().unwrap();
    assert!(frequency <= 24000.0, "{frequency}");
    let gain = sets(&simulator, BOOST_POINTER)[0][9].as_f64().unwrap();
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
        !sets(&simulator, BOOST_POINTER).is_empty()
    });
    let id = harness.state().history().newest_open().unwrap();
    harness.state_mut().request_take_back(id);
    wait_until(&mut harness, "the gain written back", |_| {
        sets(&simulator, BOOST_POINTER).len() == 2
    });
    let mut expected = vec![Value::Null; 10];
    expected[1] = json!(0.0);
    assert_eq!(sets(&simulator, BOOST_POINTER)[1], Value::Array(expected));
}

#[test]
fn the_crossovers_of_the_outputs_can_be_edited() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh750);
    // "Ausgang A" is also a heading in the device panel; the choice is a button.
    harness
        .get_by_role_and_label(Role::Button, "Ausgang A")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert_eq!(gain_fields(&harness), vec![false; 2]);
}

#[test]
fn the_kh120_eq3_can_be_edited() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    harness.get_by_label("EQ 3").click();
    harness.run_steps(2);
    assert_eq!(gain_fields(&harness), vec![false; 20]);
}

#[test]
fn the_crossover_is_shown_on_the_kh750() {
    let (_simulator, _directory, harness) = open_eq(SimModel::Kh750);
    harness.get_by_label("Weiche A");
    harness.get_by_label("Weiche B");
    // Measured: `out` is the KH 750's own driver, `out1`/`out2` the sockets A and B.
    harness.get_by_label("Subwoofer");
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
        !sets(&simulator, BOOST_POINTER).is_empty()
    });
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::HISTORY.get()).click();
    harness.run_steps(2);
    harness.get_by_label_contains(&texts::eq_change_setting("EQ 2", 2, BOOST_LABEL));
}

#[test]
fn the_strip_opens_the_eq_window() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh750);
    // With the panel closed, the strip's button is the only "EQ".
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    harness.get_by_label("Subwoofer");
}

#[test]
fn the_eq_button_waits_for_the_speakers_model() {
    // Never connected, the model is unknown: which containers it has is not known either.
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    simulator.stop();
    let mut harness = harness(path, Vec::new());
    harness.run_steps(5);
    harness.get_by_label(texts::MUTE.get());
    assert!(harness
        .query_by_role_and_label(Role::Button, texts::EQ.get())
        .is_none());
}

fn shaped(simulator: &Simulator) {
    spread(simulator);
    simulator.set_value("audio/out/eq2/gain", json!(vec![1.5; 10]));
    simulator.set_value("audio/out/eq2/boost", json!(vec![-3.0; 10]));
}

fn reset_buttons(harness: &Harness<'static, NeumannApp>) -> usize {
    harness
        .query_all_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .count()
}

#[test]
fn reset_asks_then_flattens_gain_and_boost() {
    let (simulator, _directory, mut harness) = connected_speaker_with(SimModel::Kh120Ii, shaped);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .click();
    harness.run_steps(2);
    harness.get_by_label_contains(&texts::eq_reset_question("EQ 2"));
    barrier(&simulator);
    assert!(sets(&simulator, GAIN_POINTER).is_empty());
    // Asking replaces the button by the confirming one.
    assert_eq!(reset_buttons(&harness), 1);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .click();
    wait_until(&mut harness, "both arrays written", |_| {
        !sets(&simulator, GAIN_POINTER).is_empty() && !sets(&simulator, BOOST_POINTER).is_empty()
    });
    assert_eq!(sets(&simulator, GAIN_POINTER), vec![json!(vec![0.0; 10])]);
    assert_eq!(sets(&simulator, BOOST_POINTER), vec![json!(vec![0.0; 10])]);
    assert!(sets(&simulator, FREQUENCY_POINTER).is_empty());
    let history: Vec<_> = harness.state().history().entries().cloned().collect();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].changes.len(), 2);
    assert_eq!(history[0].changes[0].before, Some(json!(vec![1.5; 10])));
}

#[test]
fn a_reset_can_be_cancelled() {
    let (simulator, _directory, mut harness) = connected_speaker_with(SimModel::Kh120Ii, shaped);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::CANCEL.get())
        .click();
    harness.run_steps(3);
    barrier(&simulator);
    assert!(sets(&simulator, GAIN_POINTER).is_empty());
    assert!(harness
        .query_by_label_contains(&texts::eq_reset_question("EQ 2"))
        .is_none());
    assert_eq!(reset_buttons(&harness), 1);
}

#[test]
fn reset_is_not_offered_on_a_crossover() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh750);
    assert_eq!(reset_buttons(&harness), 1);
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert_eq!(reset_buttons(&harness), 0);
}

#[test]
fn reset_is_not_offered_while_the_speaker_is_unreachable() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    assert_eq!(reset_buttons(&harness), 1);
    simulator.stop();
    wait_until(&mut harness, "the reset withdrawn", |harness| {
        reset_buttons(harness) == 0
    });
}

/// A band edit, then a newer mute from the strip (with the window closed, as it covers the
/// strip), then the window open again.
fn eq_change_then_mute(harness: &mut Harness<'static, NeumannApp>, simulator: &Simulator) {
    let point = harness.get_by_label(&texts::eq_band(2)).rect().center();
    drag(harness, point, egui::vec2(0.0, -40.0));
    wait_until(harness, "the band write", |_| {
        !sets(simulator, BOOST_POINTER).is_empty()
    });
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::MUTE.get())
        .click();
    wait_until(harness, "the mute write", |_| {
        !sets(simulator, "/audio/out/mute").is_empty()
    });
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(harness, texts::EQ_COMPUTED.get());
}

fn assert_band_2_written_back(simulator: &Simulator) {
    let boost = sets(simulator, BOOST_POINTER);
    assert_eq!(boost.len(), 2, "{boost:?}");
    let mut expected = vec![Value::Null; 10];
    expected[1] = json!(0.0);
    assert_eq!(boost[1], Value::Array(expected));
    assert_eq!(sets(simulator, "/audio/out/mute").len(), 1);
}

#[test]
fn eq_undo_takes_back_the_speakers_last_eq_change() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    eq_change_then_mute(&mut harness, &simulator);
    harness.get_by_label_contains(&texts::eq_change_setting("EQ 2", 2, BOOST_LABEL));
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_UNDO.get())
        .click();
    wait_until(&mut harness, "the band written back", |_| {
        sets(&simulator, BOOST_POINTER).len() == 2
    });
    assert_band_2_written_back(&simulator);
}

#[test]
fn ctrl_z_in_the_eq_window_undoes_eq_only() {
    let (simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    eq_change_then_mute(&mut harness, &simulator);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    wait_until(&mut harness, "the band written back", |_| {
        sets(&simulator, BOOST_POINTER).len() == 2
    });
    harness.run_steps(3);
    barrier(&simulator);
    assert_band_2_written_back(&simulator);
}

#[test]
fn eq_undo_is_disabled_without_eq_changes() {
    let (_simulator, _directory, harness) = open_eq(SimModel::Kh120Ii);
    let undo = harness.get_by_role_and_label(Role::Button, texts::EQ_UNDO.get());
    assert!(undo.accesskit_node().is_disabled());
}

#[test]
fn a_reset_reads_as_all_bands_in_the_history() {
    let (simulator, _directory, mut harness) = connected_speaker_with(SimModel::Kh120Ii, shaped);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    for _ in 0..2 {
        harness
            .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
            .click();
        harness.run_steps(2);
    }
    wait_until(&mut harness, "both arrays written", |_| {
        !sets(&simulator, GAIN_POINTER).is_empty() && !sets(&simulator, BOOST_POINTER).is_empty()
    });
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::HISTORY.get()).click();
    harness.run_steps(2);
    // The changes of an entry stand in the tooltip of its line.
    harness
        .get_by_label_contains(&texts::history_changes(None, 2))
        .hover();
    harness.run_steps(3);
    let one_band = texts::eq_change_setting("EQ 2", 1, GAIN_LABEL);
    assert!(harness.query_by_label_contains(&one_band).is_none());
    harness.get_by_label_contains(&texts::eq_all_bands_setting("EQ 2", GAIN_LABEL));
}

/// A KH 750 with its EQ window open, crossover protection switched off in the settings.
fn open_eq_with_open_crossovers() -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let simulator = Simulator::start(SimModel::Kh750);
    simulator.set_reply_delay(REPLY_DELAY);
    spread(&simulator);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut settings = neumann_control_core::settings::load(&path);
    settings.desk.crossovers = CrossoverGuard::Open;
    neumann_control_core::settings::save(&settings, &path).unwrap();
    let mut harness = harness(path, Vec::new());
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &simulator);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
    (simulator, directory, harness)
}

#[test]
fn with_open_crossovers_flat_is_offered_with_a_warning() {
    let (_simulator, _directory, mut harness) = open_eq_with_open_crossovers();
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert_eq!(reset_buttons(&harness), 1);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .click();
    harness.run_steps(2);
    harness.get_by_label_contains(texts::CROSSOVER_WARNING.get());
}

#[test]
fn eq_2_has_no_crossover_warning() {
    let (_simulator, _directory, mut harness) = open_eq_with_open_crossovers();
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_RESET.get())
        .click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label_contains(texts::CROSSOVER_WARNING.get())
        .is_none());
}

/// The EQ window of the strip whose name is `name`, with two speakers on the desk.
fn open_eq_of(harness: &mut Harness<'static, NeumannApp>, name: &str) {
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    let column = harness.get_by_label(name).rect().center().x;
    let button = harness
        .query_all_by_role_and_label(Role::Button, texts::EQ.get())
        .min_by(|a, b| {
            let distance = |node: &egui_kittest::Node| (node.rect().center().x - column).abs();
            distance(a).total_cmp(&distance(b))
        })
        .expect("an EQ button");
    button.click();
    wait_for_label(harness, texts::EQ_COMPUTED.get());
}

#[test]
fn another_speakers_curve_can_be_compared() {
    let (_kh120, _kh750, _directory, mut harness) = grouped(Vec::new());
    open_eq_of(&mut harness, KH750_DEVICE_NAME);
    let compared = texts::compared_curve(KH120_DEVICE_NAME);
    assert!(harness.query_by_label(&compared).is_none());
    harness
        .get_by_role_and_label(Role::CheckBox, KH120_DEVICE_NAME)
        .click();
    wait_until(&mut harness, "the compared curve", |harness| {
        harness.query_by_label(&compared).is_some()
    });
    // Output A has no counterpart on the KH 120 II, so nothing to compare there.
    harness
        .get_by_role_and_label(Role::Button, "Ausgang A")
        .click();
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::CheckBox, KH120_DEVICE_NAME)
        .is_none());
}

fn save_eq_preset(harness: &mut Harness<'static, NeumannApp>, name: &str) {
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_PRESET_SAVE.get())
        .click();
    harness.run_steps(3);
    let field = || texts::PRESET_NAME.get();
    harness
        .get_by_role_and_label(Role::TextInput, field())
        .click();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, field())
        .type_text(name);
    harness.run_steps(1);
    harness.get_by_label(texts::SAVE.get()).click();
    harness.run_steps(3);
}

#[test]
fn an_eq_preset_is_saved_and_written_back_as_one_entry() {
    let (simulator, directory, mut harness) = open_eq(SimModel::Kh120Ii);
    save_eq_preset(&mut harness, "Warm");
    let saved = neumann_control_core::settings::load(&directory.path().join("settings.json"));
    assert_eq!(saved.eq_presets.len(), 1);
    assert_eq!(saved.eq_presets[0].bands, 10);
    assert_eq!(saved.eq_presets[0].leaves.len(), 6);

    let point = harness.get_by_label(&texts::eq_band(2)).rect().center();
    drag(&mut harness, point, egui::vec2(0.0, -40.0));
    wait_until(&mut harness, "the band write", |_| {
        !sets(&simulator, BOOST_POINTER).is_empty()
    });
    harness.run_steps(3);
    let entries_before = harness.state().history().len();
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_PRESET_LOAD.get())
        .click();
    harness.run_steps(3);
    harness
        .get_by_label(&texts::eq_preset_entry("Warm", 10))
        .click();
    harness.run_steps(6);
    harness.get_by_label(&texts::eq_preset_band(2));
    harness.get_by_label(texts::WRITE_PRESET.get()).click();
    wait_until(&mut harness, "the preset's boost written", |_| {
        sets(&simulator, BOOST_POINTER).len() == 2
    });
    assert_eq!(sets(&simulator, BOOST_POINTER)[1], json!(vec![0.0; 10]));
    harness.run_steps(2);
    assert_eq!(harness.state().history().len(), entries_before + 1);
}

#[test]
fn eq_presets_of_another_band_count_are_not_offered() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    save_eq_preset(&mut harness, "Warm");
    harness.get_by_label("EQ 3").click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_PRESET_LOAD.get())
        .click();
    harness.run_steps(3);
    harness.get_by_label(texts::NO_PRESETS.get());
    assert!(harness
        .query_by_label(&texts::eq_preset_entry("Warm", 10))
        .is_none());
}

#[test]
fn a_protected_crossover_has_no_eq_presets() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh750);
    harness.get_by_role_and_label(Role::Button, texts::EQ_PRESET_SAVE.get());
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::Button, texts::EQ_PRESET_SAVE.get())
        .is_none());
}

#[test]
fn with_open_crossovers_loading_an_eq_preset_warns() {
    let (_simulator, _directory, mut harness) = open_eq_with_open_crossovers();
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_PRESET_LOAD.get())
        .click();
    harness.run_steps(3);
    harness.get_by_label_contains(texts::CROSSOVER_WARNING.get());
}

#[test]
fn a_switched_off_band_has_no_point_in_the_graph() {
    let (_simulator, _directory, harness) = open_eq_with(SimModel::Kh120Ii, |simulator| {
        let mut enabled = vec![true; 10];
        enabled[2] = false;
        simulator.set_value("audio/out/eq2/enabled", serde_json::json!(enabled));
    });
    harness.get_by_label(&texts::eq_band(2));
    assert!(harness.query_by_label(&texts::eq_band(3)).is_none());
}

#[test]
fn the_eq_display_is_chosen_and_reset_in_the_settings() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_reply_delay(REPLY_DELAY);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut stored = neumann_control_core::settings::load(&path);
    stored
        .eq_display
        .band_colors
        .insert("3".to_owned(), "#ff0000".to_owned());
    neumann_control_core::settings::save(&stored, &path).unwrap();
    let mut harness = harness(path.clone(), Vec::new());
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(4);
    harness
        .get_by_label(texts::BAND_CURVES_CHOSEN.get())
        .click();
    harness.run_steps(2);
    // The dialog scrolls; the button lies below what it shows at first.
    harness
        .get_by_label(texts::DEFAULT_COLORS.get())
        .scroll_to_me();
    harness.run_steps(3);
    harness.get_by_label(texts::DEFAULT_COLORS.get()).click();
    harness.run_steps(2);
    let stored = neumann_control_core::settings::load(&path).eq_display;
    assert_eq!(
        stored.band_curves,
        neumann_control_core::settings::BandCurves::ChosenAndHovered
    );
    assert!(stored.band_colors.is_empty(), "{:?}", stored.band_colors);
}

#[test]
fn a_full_reset_switches_every_band_off_and_back_to_the_start_values() {
    let (simulator, _directory, mut harness) = open_eq_with(SimModel::Kh120Ii, |simulator| {
        simulator.set_value("audio/out/eq2/gain", json!(vec![1.5; 10]));
        simulator.set_value("audio/out/eq2/q", json!(vec![2.0; 10]));
    });
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get())
        .click();
    harness.run_steps(2);
    harness.get_by_label_contains(&texts::eq_full_reset_question("EQ 2"));
    barrier(&simulator);
    assert!(sets(&simulator, "/audio/out/eq2/enabled").is_empty());
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get())
        .click();
    wait_until(&mut harness, "all six leaves written", |_| {
        ["enabled", "type", "frequency", "q", "gain", "boost"]
            .iter()
            .all(|leaf| !sets(&simulator, &format!("/audio/out/eq2/{leaf}")).is_empty())
    });
    assert_eq!(
        sets(&simulator, "/audio/out/eq2/enabled"),
        vec![json!(vec![false; 10])]
    );
    assert_eq!(
        sets(&simulator, FREQUENCY_POINTER),
        vec![json!(vec![100.0; 10])]
    );
    assert_eq!(
        sets(&simulator, "/audio/out/eq2/q"),
        vec![json!(vec![0.7; 10])]
    );
    assert_eq!(sets(&simulator, GAIN_POINTER), vec![json!(vec![0.0; 10])]);
    let history: Vec<_> = harness.state().history().entries().cloned().collect();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].changes.len(), 6);
    assert!(history[0].changes[0].path.to_string().ends_with("enabled"));
}

#[test]
fn a_full_reset_is_not_offered_on_a_protected_crossover() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh750);
    harness.get_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get());
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get())
        .is_none());
}

/// The EQ window's file dialog hands out `files` from now on.
fn use_files(harness: &mut Harness<'static, NeumannApp>, files: &FakeFiles) {
    harness.state_mut().set_file_dialog(Box::new(files.clone()));
}

/// Writes `text` as `name` into `directory` and imports it.
fn import_file(
    harness: &mut Harness<'static, NeumannApp>,
    directory: &tempfile::TempDir,
    name: &str,
    text: &str,
) {
    let path = directory.path().join(name);
    std::fs::write(&path, text).unwrap();
    use_files(
        harness,
        &FakeFiles {
            open: Some(path),
            ..FakeFiles::default()
        },
    );
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_IMPORT.get())
        .click();
    // The preview is a modal; it centres itself over a few frames.
    harness.run_steps(6);
}

#[test]
fn an_apo_file_is_previewed_then_written_as_one_history_entry() {
    // The whole frequency list is written, so every band must be in the speakers' range (the
    // spread's tenth band is above it).
    let (simulator, directory, mut harness) = open_eq_with(SimModel::Kh120Ii, |simulator| {
        let frequencies: Vec<f64> = (0..10).map(|band| 40.0 * 2f64.powi(band)).collect();
        simulator.set_value("audio/out/eq2/frequency", json!(frequencies));
    });
    let text = "Preamp: -3 dB\r\nFilter 1: ON PK Fc 120 Hz Gain -4 dB Q 2\r\n";
    import_file(&mut harness, &directory, "room.txt", text);
    harness.get_by_label(&texts::eq_import_from("room.txt"));
    harness.get_by_label(&texts::eq_import_ignored(&["Preamp".to_owned()]));
    harness.get_by_label(&texts::eq_preset_band(1));
    harness.get_by_label(&texts::eq_preset_band(10));
    barrier(&simulator);
    assert!(sets(&simulator, "/audio/out/eq2/enabled").is_empty());
    harness
        .get_by_role_and_label(Role::Button, texts::WRITE_PRESET.get())
        .click();
    wait_until(&mut harness, "the import written", |_| {
        !sets(&simulator, BOOST_POINTER).is_empty()
    });
    let mut enabled = vec![false; 10];
    enabled[0] = true;
    assert_eq!(
        sets(&simulator, "/audio/out/eq2/enabled"),
        vec![json!(enabled)]
    );
    assert_eq!(sets(&simulator, BOOST_POINTER)[0][0], json!(-4.0));
    assert_eq!(sets(&simulator, FREQUENCY_POINTER)[0][0], json!(120.0));
    let history: Vec<_> = harness.state().history().entries().cloned().collect();
    assert_eq!(history.len(), 1);
    assert!(history[0].changes[0].path.to_string().ends_with("enabled"));
}

#[test]
fn a_file_the_speaker_would_refuse_writes_nothing() {
    let (simulator, directory, mut harness) = open_eq(SimModel::Kh120Ii);
    // 5 Hz is below the speakers' 10 Hz.
    let text = "Filter 1: ON PK Fc 5 Hz Gain -4 dB Q 2\n";
    import_file(&mut harness, &directory, "low.txt", text);
    let write = harness.get_by_role_and_label(Role::Button, texts::WRITE_PRESET.get());
    assert!(write.accesskit_node().is_disabled());
    harness
        .get_by_role_and_label(Role::Button, texts::CANCEL.get())
        .click();
    harness.run_steps(2);
    barrier(&simulator);
    assert!(sets(&simulator, "/audio/out/eq2/enabled").is_empty());
    assert!(sets(&simulator, FREQUENCY_POINTER).is_empty());
}

#[test]
fn a_shelf_by_slope_in_the_file_is_named_and_refused() {
    let (_simulator, directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let text =
        "Filter 1: ON PK Fc 120 Hz Gain -4 dB Q 2\nFilter 2: ON LSC 10.8 dB Fc 80 Hz Gain 3 dB\n";
    import_file(&mut harness, &directory, "shelf.txt", text);
    let error = eq_file::ImportError::Line {
        line: 2,
        problem: eq_file::LineProblem::UnsupportedForm("LSC 10.8".to_owned()),
    };
    harness.get_by_label(&texts::eq_import_error(&error));
    let write = harness.get_by_role_and_label(Role::Button, texts::WRITE_PRESET.get());
    assert!(write.accesskit_node().is_disabled());
}

#[test]
fn import_is_not_offered_on_a_protected_crossover_but_export_is() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh750);
    harness.get_by_role_and_label(Role::Button, texts::EQ_IMPORT.get());
    harness.get_by_label("Weiche A").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::Button, texts::EQ_IMPORT.get())
        .is_none());
    harness.get_by_role_and_label(Role::Button, texts::EQ_EXPORT.get());
}

#[test]
fn the_export_saves_the_chosen_eq_as_apo_lines() {
    let (_simulator, directory, mut harness) = open_eq(SimModel::Kh120Ii);
    let path = directory.path().join("export.txt");
    let files = FakeFiles {
        save: Some(path.clone()),
        ..FakeFiles::default()
    };
    use_files(&mut harness, &files);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_EXPORT.get())
        .click();
    harness.run_steps(2);
    assert_eq!(*files.suggested.lock().unwrap(), ["KH 120 II EQ 2.txt"]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("# Neumann Control EQ export: EQ 2, 10 bands"),
        "{text}"
    );
    assert!(text.contains("Filter 1: ON PK Fc 50 Hz"), "{text}");
    harness.get_by_label(&texts::eq_export_report("export.txt", &[])[0]);
}

#[test]
fn a_cancelled_file_dialog_changes_nothing() {
    let (_simulator, _directory, mut harness) = open_eq(SimModel::Kh120Ii);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_IMPORT.get())
        .click();
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_EXPORT.get())
        .click();
    harness.run_steps(6);
    assert!(harness
        .query_by_label(texts::EQ_IMPORT_TITLE.get())
        .is_none());
    assert!(harness
        .query_by_label_contains(&texts::eq_export_report("", &[])[0])
        .is_none());
}

/// The KH 120 II refuses a request of more than 29 JSON tokens (413), and every write of its
/// 20-band EQ 3 with the transaction id has more: "Reset" there was refused on the speaker.
#[test]
fn a_full_reset_of_the_twenty_band_eq_3_is_taken() {
    let (simulator, _directory, mut harness) = open_eq_with(SimModel::Kh120Ii, |simulator| {
        simulator.set_value("audio/out/eq3/enabled", json!(vec![true; 20]));
        simulator.set_value("audio/out/eq3/q", json!(vec![2.0; 20]));
    });
    harness.get_by_label("EQ 3").click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get())
        .click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ_FULL_RESET.get())
        .click();
    wait_until(&mut harness, "all six leaves of EQ 3 written", |_| {
        ["enabled", "type", "frequency", "q", "gain", "boost"]
            .iter()
            .all(|leaf| !sets(&simulator, &format!("/audio/out/eq3/{leaf}")).is_empty())
    });
    barrier(&simulator);
    assert_eq!(
        simulator.value("audio/out/eq3/enabled"),
        Some(json!(vec![false; 20]))
    );
    assert_eq!(
        simulator.value("audio/out/eq3/q"),
        Some(json!(vec![0.7; 20]))
    );
}
