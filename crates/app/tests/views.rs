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
fn strip_shows_name_and_level_and_panel_the_masked_serial() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(KH120_DEVICE_NAME);
    harness.get_by_label(&readout(100.0));
    harness.get_by_label(&texts::speaker_count(1));
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.get_by_label_contains("KH 120 II ·");
    harness.get_by_label_contains(&mask_serial(SIMULATED_SERIAL));
    assert!(harness.query_by_label_contains(SIMULATED_SERIAL).is_none());
}

#[test]
fn fader_drag_sends_once_on_release() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let rect = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect();
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
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
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
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
    harness.drag_at(start);
    for frame in 1..=5 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    let dragged = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
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
    let center = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
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
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
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
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
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
    harness.get_by_label(texts::MUTE.get()).click();
    wait_until(&mut harness, "mute on the device", |_| {
        simulator.value("audio/out/mute") == Some(Value::Bool(true))
    });
}

#[test]
fn identify_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::IDENTIFY.get()).click();
    wait_until(&mut harness, "identification on the device", |_| {
        simulator.value("device/identification/visual") == Some(Value::Bool(true))
    });
}

#[test]
fn panel_opens_from_strip_name_and_closes_on_escape() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.get_by_label(texts::CLOSE.get());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::ALL_VALUES.get()).is_none());
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::ALL_VALUES.get()).is_none());
}

#[test]
fn panel_closes_when_its_speaker_goes_away() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    simulator.stop();
    wait_until(&mut harness, "the panel to close", |harness| {
        harness.query_by_label(texts::ALL_VALUES.get()).is_none()
    });
}

#[test]
fn unreachable_card_is_disabled() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    simulator.stop();
    wait_until(&mut harness, "the unreachable text", |harness| {
        harness.query_by_label(texts::UNREACHABLE.get()).is_some()
    });
    assert!(harness
        .get_by_label(texts::MUTE.get())
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_label(texts::IDENTIFY.get())
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
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
    open_panel(&mut harness, device_name(model));
    harness.get_by_label(texts::ALL_VALUES.get()).click();
    wait_until(&mut harness, "the loaded table", |harness| {
        harness
            .query_by_label(texts::ALL_VALUES_PATH_COLUMN.get())
            .is_some()
    });
    (simulator, directory, harness)
}

fn filter_all_values(harness: &mut Harness<'static, NeumannApp>, text: &str) {
    harness
        .get_by_label(texts::ALL_VALUES_FILTER_LABEL.get())
        .click();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label(texts::ALL_VALUES_FILTER_LABEL.get())
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
    harness.get_by_label(texts::ALL_VALUES_KNOWN.get());
    filter_all_values(&mut harness, "restore");
    harness.get_by_label("device/restore");
    assert!(harness
        .query_by_label(texts::ALL_VALUES_KNOWN.get())
        .is_none());
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
    harness.get_by_label(texts::ALL_VALUES_FAILED.get());
    assert!(harness.query_by_label_contains("fe12:3456").is_none());
    assert!(harness
        .query_by_label_contains("connection error")
        .is_none());
}

#[test]
fn strip_content_stacks_top_down() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let second = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(
        directory.path(),
        vec![manual_for(&first), manual_for(&second)],
    );
    let mut harness = harness(path, Vec::new());
    wait_until(&mut harness, "both strips connected", |harness| {
        harness
            .query_all_by_role_and_label(Role::Slider, texts::LEVEL.get())
            .count()
            == 2
            && harness.query_by_label(KH120_DEVICE_NAME).is_some()
            && harness.query_by_label(KH750_DEVICE_NAME).is_some()
    });
    harness.run_steps(3);
    let names = [
        harness.get_by_label(KH120_DEVICE_NAME).rect(),
        harness.get_by_label(KH750_DEVICE_NAME).rect(),
    ];
    let mutes: Vec<_> = harness
        .query_all_by_label(texts::MUTE.get())
        .map(|node| node.rect())
        .collect();
    let delays: Vec<_> = harness
        .query_all_by_role_and_label(Role::Slider, texts::KNOB_DELAY.get())
        .map(|node| node.rect())
        .collect();
    assert_eq!(mutes.len(), 2);
    assert_eq!(delays.len(), 2);
    for (index, name) in names.iter().enumerate() {
        let strip = if mutes[0].center().x < mutes[1].center().x {
            index
        } else {
            1 - index
        };
        assert!(name.bottom() <= delays[strip].top(), "name above the knobs");
        assert!(
            delays[strip].bottom() <= mutes[strip].top(),
            "knobs above Stumm"
        );
    }
    assert!(
        (names[0].top() - names[1].top()).abs() < 1.0,
        "strips start at the same height: {names:?}"
    );
}

/// Point on a circle of `radius` around `center` at `degrees` clockwise from the top.
fn around(center: egui::Pos2, radius: f32, degrees: f32) -> egui::Pos2 {
    let radians = degrees.to_radians();
    center + radius * egui::vec2(radians.sin(), -radians.cos())
}

/// Presses at `from` degrees on the knob's rim and moves clockwise to `to` without releasing.
fn turn_knob(
    harness: &mut Harness<'static, NeumannApp>,
    label: &str,
    from: f32,
    to: f32,
) -> egui::Pos2 {
    let center = harness
        .get_by_role_and_label(Role::Slider, label)
        .rect()
        .center();
    let radius = 14.0;
    harness.drag_at(around(center, radius, from));
    let steps = 6;
    for step in 1..=steps {
        let degrees = from + (to - from) * step as f32 / steps as f32;
        harness.hover_at(around(center, radius, degrees));
        harness.step();
    }
    around(center, radius, to)
}

#[test]
fn strip_shows_leds_knobs_fader_and_buttons() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh120Ii);
    for label in [
        texts::LED_CONNECTION,
        texts::LED_CLIP,
        texts::LED_WARNING,
        texts::LED_LOCAL,
    ] {
        harness.get_by_label(label.get());
    }
    harness.get_by_role_and_label(Role::Slider, texts::KNOB_DELAY.get());
    harness.get_by_role_and_label(Role::Slider, texts::KNOB_STANDBY.get());
    harness.get_by_label(texts::MUTE.get());
    harness.get_by_label(texts::IDENTIFY.get());
    harness.get_by_label(&readout(100.0));
}

#[test]
fn kh750_standby_knob_is_read_only() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    let standby = harness.get_by_role_and_label(Role::Slider, texts::KNOB_STANDBY.get());
    assert!(standby.accesskit_node().is_disabled());
    let delay = harness.get_by_role_and_label(Role::Slider, texts::KNOB_DELAY.get());
    assert!(!delay.accesskit_node().is_disabled());
}

#[test]
fn knob_drag_writes_once_on_release() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let end = turn_knob(&mut harness, texts::KNOB_DELAY.get(), 90.0, 150.0);
    barrier(&simulator);
    assert!(
        delay_sets(&simulator).is_empty(),
        "a Set went out while turning"
    );
    harness.drop_at(end);
    wait_until(&mut harness, "the delay Set", |_| {
        !delay_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = delay_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0].as_i64().is_some_and(|delay| delay > 0), "{sets:?}");
}

#[test]
fn escape_during_knob_drag_sends_nothing() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let end = turn_knob(&mut harness, texts::KNOB_DELAY.get(), 90.0, 150.0);
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.drop_at(end);
    harness.run_steps(10);
    barrier(&simulator);
    assert!(delay_sets(&simulator).is_empty());
}

#[test]
fn knob_wheel_writes_after_idle() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let center = harness
        .get_by_role_and_label(Role::Slider, texts::KNOB_DELAY.get())
        .rect()
        .center();
    harness.hover_at(center);
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    barrier(&simulator);
    assert!(delay_sets(&simulator).is_empty(), "the wheel wrote at once");
    wait_until(&mut harness, "the wheel Set", |_| {
        !delay_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    assert_eq!(delay_sets(&simulator), vec![serde_json::json!(58)]);
}

#[test]
fn desk_scrolls_when_narrow() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let second = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(
        directory.path(),
        vec![manual_for(&first), manual_for(&second)],
    );
    let mut harness = harness(path, Vec::new());
    harness.set_size(egui::vec2(160.0, 700.0));
    wait_until(&mut harness, "both strips", |harness| {
        harness.query_by_label(KH120_DEVICE_NAME).is_some()
            && harness.query_by_label(KH750_DEVICE_NAME).is_some()
    });
    harness.run_steps(3);
    let first_strip = harness.get_by_label(KH120_DEVICE_NAME).rect();
    let second_strip = harness.get_by_label(KH750_DEVICE_NAME).rect();
    assert!(
        first_strip.right() <= second_strip.left() || second_strip.right() <= first_strip.left(),
        "strips overlap: {first_strip:?} {second_strip:?}"
    );
    harness.get_by_role(Role::ScrollBar);
}

fn open_settings(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_label(texts::SETTINGS.get()).click();
    wait_for_label(harness, texts::THEME.get());
}

#[test]
fn settings_dialog_switches_theme_and_saves_on_change() {
    let (_simulator, directory, mut harness) = connected_card(SimModel::Kh120Ii);
    open_settings(&mut harness);
    // The harness starts dark, so light is the choice that shows an effect.
    harness.get_by_label(texts::THEME_LIGHT.get()).click();
    harness.run_steps(2);
    let preference = harness.ctx.options(|options| options.theme_preference);
    assert_eq!(preference, egui::ThemePreference::Light);
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let saved = neumann_control_core::settings::load(&path);
    assert_eq!(
        saved.desk.theme,
        neumann_control_core::settings::ThemeChoice::Light
    );
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::THEME.get()).is_none());
}

#[test]
fn english_applies_at_once_and_is_saved() {
    let (_simulator, directory, mut harness) = connected_card(SimModel::Kh120Ii);
    open_settings(&mut harness);
    harness.get_by_label(texts::LANGUAGE_ENGLISH).click();
    harness.run_steps(2);
    // Literal English on purpose: `texts` follows the language the app just set.
    harness.get_by_label("Appearance");
    harness.get_by_label("Scan");
    harness.get_by_label("Mute");
    harness.get_by_label("Close").click();
    harness.run_steps(2);
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let saved = neumann_control_core::settings::load(&path);
    assert_eq!(
        saved.desk.language,
        neumann_control_core::settings::Language::English
    );
}

#[test]
fn toolbar_shows_average_response_time_until_turned_off() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    wait_until(&mut harness, "the average response time", |harness| {
        harness.query_by_label_contains("Ø ").is_some()
    });
    open_settings(&mut harness);
    harness
        .get_by_label(texts::SHOW_RESPONSE_TIMES.get())
        .click();
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("Ø ").is_none());
}

#[test]
fn knob_value_is_shown_beside_it_while_dragging() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let unit = " ms";
    let shown = |harness: &Harness<'static, NeumannApp>| {
        harness
            .query_all_by_label_contains(unit)
            .filter(|node| node.accesskit_node().role() == Role::Label)
            // The delay has decimals; the response time ("14 ms") has none.
            .filter(|node| {
                // egui puts a label's text into the node's value.
                node.accesskit_node()
                    .value()
                    .is_some_and(|text| text.contains('.'))
            })
            .map(|node| node.rect())
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(&harness).len(), 1);
    let end = turn_knob(&mut harness, texts::KNOB_DELAY.get(), 90.0, 150.0);
    // Under the knob and beside it, each on one line.
    let labels = shown(&harness);
    assert_eq!(labels.len(), 2);
    assert!(
        labels.iter().all(|rect| rect.width() > rect.height()),
        "{labels:?}"
    );
    harness.key_press(egui::Key::Escape);
    harness.drop_at(end);
    harness.run_steps(2);
    assert_eq!(shown(&harness).len(), 1);
}

/// How far the level moves while dragging the fader `points` down, without releasing.
fn level_change_while_dragging(
    harness: &mut Harness<'static, NeumannApp>,
    points: f32,
    modifiers: egui::Modifiers,
) -> f64 {
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
    let before = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .accesskit_node()
        .numeric_value()
        .unwrap();
    harness.drag_at(start);
    // After the press, which carries no modifiers of its own.
    harness.event(egui::Event::ModifiersChanged(modifiers));
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, points * frame as f32 / 4.0));
        harness.step();
    }
    let during = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .accesskit_node()
        .numeric_value()
        .unwrap();
    harness.key_press(egui::Key::Escape);
    harness.drop_at(start);
    harness.run_steps(2);
    before - during
}

#[test]
fn fader_moves_ten_times_finer_with_shift() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let coarse = level_change_while_dragging(&mut harness, 40.0, egui::Modifiers::NONE);
    let fine = level_change_while_dragging(&mut harness, 40.0, egui::Modifiers::SHIFT);
    assert!(coarse > 10.0, "coarse {coarse}");
    assert!(
        fine > 0.0 && fine <= coarse / 5.0,
        "coarse {coarse}, fine {fine}"
    );
}

#[test]
fn double_click_on_the_level_readout_enters_a_value() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let position = harness.get_by_label(&readout(100.0)).rect().center();
    double_click_at(&mut harness, position);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("87.5");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the typed level", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(5);
    barrier(&simulator);
    assert_eq!(level_sets(&simulator), vec![87.5]);
}

#[test]
fn double_click_on_the_delay_knob_takes_milliseconds() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let position = harness
        .get_by_role_and_label(Role::Slider, texts::KNOB_DELAY.get())
        .rect()
        .center();
    double_click_at(&mut harness, position);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("10");
    harness.key_press(egui::Key::Enter);
    wait_until(&mut harness, "the typed delay", |_| {
        !delay_sets(&simulator).is_empty()
    });
    harness.run_steps(5);
    barrier(&simulator);
    assert_eq!(delay_sets(&simulator), vec![serde_json::json!(480)]);
}

#[test]
fn meter_groups_are_labelled_input_and_output() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    harness.get_by_label(texts::METER_INPUT_SHORT.get());
    harness.get_by_label(texts::METER_OUTPUT_SHORT.get());
    // The KH 120 II names its input by the rear switch instead.
    let (_other, _other_directory, kh120) = connected_card(SimModel::Kh120Ii);
    kh120.get_by_label(texts::SOURCE_ANALOG.get());
    assert!(kh120
        .query_by_label(texts::METER_OUTPUT_SHORT.get())
        .is_none());
}

#[test]
fn meter_captions_are_centred_under_their_meters() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    for (caption, group, last) in [
        (texts::METER_INPUT_SHORT, texts::INPUT_METERS, "B"),
        (texts::METER_OUTPUT_SHORT, texts::OUTPUT_METERS, "Sub"),
    ] {
        let first = harness
            .get_by_label(&texts::meter_label(group.get(), "A"))
            .rect();
        let last = harness
            .get_by_label(&texts::meter_label(group.get(), last))
            .rect();
        let middle = (first.left() + last.right()) / 2.0;
        let centre = harness.get_by_label(caption.get()).rect().center().x;
        assert!(
            (centre - middle).abs() < 2.0,
            "{caption}: {centre} vs {middle}"
        );
    }
}

#[test]
fn meters_name_their_channels_as_the_speaker_does() {
    // The KH 750 describes its outputs as "[A, B, subwoofer]", its inputs as "[A,B]".
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    for (group, channel) in [
        (texts::INPUT_METERS, "A"),
        (texts::INPUT_METERS, "B"),
        (texts::OUTPUT_METERS, "A"),
        (texts::OUTPUT_METERS, "B"),
        (texts::OUTPUT_METERS, "Sub"),
    ] {
        harness.get_by_label(&texts::meter_label(group.get(), channel));
    }
    assert_eq!(harness.query_all_by_label("Sub").count(), 1);
}

#[test]
fn fader_and_meter_groups_line_up_at_the_top() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    let top = |label: String| harness.get_by_label(&label).rect().top();
    let input = top(texts::meter_label(texts::INPUT_METERS.get(), "A"));
    let output = top(texts::meter_label(texts::OUTPUT_METERS.get(), "A"));
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .top();
    assert!(
        (input - output).abs() < 1.0,
        "input {input}, output {output}"
    );
    assert!((input - fader).abs() < 1.0, "input {input}, fader {fader}");
}

#[test]
fn channel_names_do_not_touch() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh750);
    // The output B is the right one of the two.
    let b = harness
        .query_all_by_label("B")
        .map(|node| node.rect())
        .max_by(|first, second| first.left().total_cmp(&second.left()))
        .unwrap();
    let sub = harness.get_by_label("Sub").rect();
    assert!(sub.left() > b.right(), "B {b:?}, Sub {sub:?}");
}

#[test]
fn played_input_channels_are_marked_with_the_source() {
    let (simulator, _directory, analog) = connected_card(SimModel::Kh120Ii);
    analog.get_by_label(texts::SOURCE_ANALOG.get());
    assert_eq!(
        analog
            .query_all_by_label(texts::PLAYED_CHANNEL.get())
            .count(),
        2
    );
    drop((simulator, analog));
    // The switch is polled every 30 s, so the speaker reports AES3 L from the start.
    let left = Simulator::start(SimModel::Kh120Ii);
    left.set_value("ui/input_select", serde_json::json!("AES3 L"));
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&left)]);
    let mut aes3 = harness(path, Vec::new());
    wait_for_level(&mut aes3, 100.0);
    wait_for_initial_read(&mut aes3, &left);
    aes3.get_by_label(texts::SOURCE_AES3_LEFT.get());
    assert_eq!(
        aes3.query_all_by_label(texts::PLAYED_CHANNEL.get()).count(),
        1
    );
    let (_other, _other_directory, kh750) = connected_card(SimModel::Kh750);
    kh750.get_by_label(texts::METER_INPUT_SHORT.get());
    assert_eq!(
        kh750
            .query_all_by_label(texts::PLAYED_CHANNEL.get())
            .count(),
        0
    );
}

#[test]
fn input_source_caption_stays_on_one_line() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh120Ii);
    let caption = harness.get_by_label(texts::SOURCE_ANALOG.get()).rect();
    assert!(caption.width() > caption.height(), "{caption:?}");
    let level = harness.get_by_label(&readout(100.0)).rect();
    assert!(
        caption.bottom() <= level.top(),
        "caption {caption:?}, level {level:?}"
    );
}
