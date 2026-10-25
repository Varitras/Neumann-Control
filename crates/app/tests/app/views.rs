//! The speaker strip in the real window: values, fader, buttons, unreachable state.

use crate::support::*;
use eframe::egui::{self, accesskit::Role, MouseWheelUnit, TouchPhase};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::panel_tabs::PanelTab;
use neumann_control_app::paths;
use neumann_control_app::state::{SpeakerKey, SpeakerState};
use neumann_control_app::texts;
use neumann_control_app::views::{all_values, Opening};
use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::{self, Language, LanguageChoice};
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::Event;
use serde_json::Value;

#[test]
fn strip_shows_name_and_level_and_panel_the_masked_serial() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
            source: egui::MouseWheelSource::Wheel,
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
    harness.hover_at(start);
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::MUTE.get()).click();
    wait_until(&mut harness, "mute on the device", |_| {
        simulator.value("audio/out/mute") == Some(Value::Bool(true))
    });
}

#[test]
fn identify_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::IDENTIFY.get()).click();
    wait_until(&mut harness, "identification on the device", |_| {
        simulator.value("device/identification/visual") == Some(Value::Bool(true))
    });
}

#[test]
fn panel_opens_from_strip_name_and_closes_on_escape() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::TAB_OVERVIEW.get()).is_none());
    // The window's own ✕ closes it; the panel has no second close button.
    open_panel(&mut harness, KH120_DEVICE_NAME);
    assert!(harness.query_by_label(texts::CLOSE.get()).is_none());
}

/// From the top of the panel's embedded window down to the middle of its title bar.
const TITLE_BAR_MIDDLE: f32 = 10.0;
const PANEL_MOVE: f32 = 520.0;

fn drag_from_to(harness: &mut Harness<'static, NeumannApp>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    for step in 1..=DRAG_STEPS {
        harness.hover_at(from + (to - from) * (step as f32 / DRAG_STEPS as f32));
        harness.run_steps(1);
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
}

const DRAG_STEPS: usize = 5;

#[test]
fn the_details_button_opens_the_panel() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::DETAILS.get())
        .click();
    wait_for_label(&mut harness, texts::TAB_OVERVIEW.get());
}

/// Each speaker's panel is a window of its own; opening a second leaves the first open.
#[test]
fn two_panels_stay_open_side_by_side() {
    let (_simulators, _directory, mut harness) = desk_of(
        &[SimModel::Kh120Ii, SimModel::Kh750],
        std::collections::BTreeMap::new(),
    );
    open_panel(&mut harness, KH120_DEVICE_NAME);
    // In the test window the panel is embedded over the desk: move it off the other strip.
    let window = harness
        .get_by_label(&texts::panel_window_title(KH120_DEVICE_NAME))
        .rect();
    let title = egui::pos2(window.center().x, window.top() + TITLE_BAR_MIDDLE);
    drag_from_to(&mut harness, title, title + egui::vec2(PANEL_MOVE, 0.0));
    harness.get_by_label(KH750_DEVICE_NAME).click();
    wait_until(&mut harness, "two panels", |harness| {
        harness
            .query_all_by_label(texts::TAB_OVERVIEW.get())
            .count()
            == 2
    });
}

/// A window beside the desk stays open while you work on the desk.
#[test]
fn a_click_on_the_desk_keeps_the_panel_open() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    let below_the_strip = egui::pos2(20.0, EXPANDED_WINDOW_SIZE[1] - 20.0);
    harness.hover_at(below_the_strip);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: below_the_strip,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run_steps(2);
    harness.get_by_label(texts::TAB_OVERVIEW.get());
}

#[test]
fn panel_closes_when_its_speaker_goes_away() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    simulator.stop();
    wait_until(&mut harness, "the panel to close", |harness| {
        harness.query_by_label(texts::TAB_OVERVIEW.get()).is_none()
    });
}

/// Never reached since the start, a speaker has no readings: its strip still shows two empty
/// input meters, so their caption stays below them instead of on the scale.
#[test]
fn a_speaker_never_reached_keeps_its_meters_and_caption_apart() {
    let reserved = Simulator::start(SimModel::Kh120Ii);
    let entry = manual_for(&reserved);
    reserved.stop();
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![entry]);
    let mut harness = harness(path, Vec::new());
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    let meters: Vec<egui::Rect> = harness
        .query_all_by_role(Role::ProgressIndicator)
        .map(|node| node.rect())
        .collect();
    assert_eq!(meters.len(), 2, "input A and B");
    let caption = harness.get_by_label(texts::METER_INPUT_SHORT.get()).rect();
    assert!(
        caption.top() >= meters[0].bottom(),
        "{caption:?} {meters:?}"
    );
}

#[test]
fn unreachable_strip_is_disabled() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
/// A refusal text as the worker produces it from a lost connection, address included.
const RAW_FAILURE: &str = "connection error: could not read from fe80::2a36:38ff:fe12:3456%7";

fn all_values_window(
    model: SimModel,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let (simulator, directory, mut harness) = connected_speaker(model);
    open_panel_on(&mut harness, device_name(model), PanelTab::Device);
    scroll_to_and_click(&mut harness, texts::ALL_VALUES.get());
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

/// A window of its own like the EQ and the panel: Escape closes it.
#[test]
fn all_values_closes_on_escape() {
    let (_simulator, _directory, mut harness) = all_values_window(SimModel::Kh120Ii);
    let title = texts::all_values_title(KH120_DEVICE_NAME);
    harness.get_by_label(&title);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(&title).is_none());
}

#[test]
fn all_values_lists_50_leaves_for_kh120ii() {
    let (_simulator, _directory, harness) = all_values_window(SimModel::Kh120Ii);
    harness.get_by_label(&texts::all_values_title(KH120_DEVICE_NAME));
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
    harness.get_by_label(paths::STANDBY_ENABLED);
    assert!(harness.query_by_label("audio/out/level").is_none());
}

#[test]
fn known_column_marks_catalog_paths() {
    let (_simulator, _directory, mut harness) = all_values_window(SimModel::Kh120Ii);
    filter_all_values(&mut harness, paths::STANDBY_ENABLED);
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
        all_values::show(ui.ctx(), &mut speaker, catalog, Opening::Default);
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
        .query_all_by_role_and_label(Role::Slider, DELAY_LABEL)
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

#[test]
fn strip_shows_leds_knobs_fader_and_buttons() {
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh120Ii);
    for label in [
        texts::LED_CONNECTION,
        texts::LED_CLIP,
        texts::LED_WARNING,
        texts::LED_LOCAL,
    ] {
        harness.get_by_label(label.get());
    }
    harness.get_by_role_and_label(Role::Slider, DELAY_LABEL);
    harness.get_by_role_and_label(Role::Slider, STANDBY_TIME_LABEL);
    harness.get_by_label(texts::MUTE.get());
    harness.get_by_label(texts::IDENTIFY.get());
    harness.get_by_label(&readout(100.0));
}

#[test]
fn kh750_standby_knob_can_be_turned() {
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
    let standby = harness.get_by_role_and_label(Role::Slider, STANDBY_TIME_LABEL);
    assert!(!standby.accesskit_node().is_disabled());
    let delay = harness.get_by_role_and_label(Role::Slider, DELAY_LABEL);
    assert!(!delay.accesskit_node().is_disabled());
}

#[test]
fn knob_drag_writes_once_on_release() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let end = turn_knob(&mut harness, DELAY_LABEL, 90.0, 150.0);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let end = turn_knob(&mut harness, DELAY_LABEL, 90.0, 150.0);
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.drop_at(end);
    harness.run_steps(10);
    barrier(&simulator);
    assert!(delay_sets(&simulator).is_empty());
}

#[test]
fn knob_wheel_writes_after_idle() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let center = harness
        .get_by_role_and_label(Role::Slider, DELAY_LABEL)
        .rect()
        .center();
    harness.hover_at(center);
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
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

/// Scrolled to the foot of a strip in a small window, "Details" is not under the horizontal
/// scroll bar (egui's bars float over the content by default).
#[test]
fn the_scroll_bar_does_not_cover_a_strips_foot() {
    let (_simulators, _directory, mut harness) = desk_of(
        &[SimModel::Kh120Ii, SimModel::Kh750],
        std::collections::BTreeMap::new(),
    );
    harness.set_size(egui::vec2(300.0, 500.0));
    harness.run_steps(2);
    harness
        .query_all_by_role_and_label(Role::Button, texts::DETAILS.get())
        .next()
        .expect("a details button")
        .scroll_to_me();
    harness.run_steps(SCROLL_FRAMES);
    let details = harness
        .query_all_by_role_and_label(Role::Button, texts::DETAILS.get())
        .next()
        .unwrap()
        .rect();
    let bars: Vec<egui::Rect> = harness
        .query_all_by_role(Role::ScrollBar)
        .map(|node| node.rect())
        .collect();
    assert!(!bars.is_empty());
    for bar in bars {
        assert!(!bar.intersects(details), "{bar:?} covers {details:?}");
    }
}

/// Without a saved window the window is made as large as the desk needs: here one strip and the
/// master, far less than the first 900 points, with the strip's foot in the window. (kittest
/// carries out the window's size commands itself.)
#[test]
fn a_first_window_fits_the_desk() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let first_start = neumann_control_core::settings::Settings {
        manual: vec![manual_for(&simulator)],
        ..Default::default()
    };
    neumann_control_core::settings::save(&first_start, &path).unwrap();
    let mut harness = harness(path, Vec::new());
    wait_for_label(&mut harness, texts::DETAILS.get());
    harness.run_steps(5);
    let window = harness.ctx.content_rect();
    assert!(window.width() < 900.0, "{window:?}");
    let details = harness
        .get_by_role_and_label(Role::Button, texts::DETAILS.get())
        .rect();
    assert!(window.contains_rect(details), "{details:?} in {window:?}");
}

fn open_settings(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_label(texts::SETTINGS.get()).click();
    wait_for_label(harness, texts::THEME.get());
}

/// A window of their own, the settings close on Escape as the other windows do.
#[test]
fn escape_closes_the_settings_window() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_settings(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::THEME.get()).is_none());
}

#[test]
fn settings_dialog_switches_theme_and_saves_on_change() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    scroll_to_and_click(&mut harness, texts::CLOSE.get());
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::THEME.get()).is_none());
}

/// Unticking "Protect crossovers" opens them in the saved settings, ticking it again protects.
#[test]
fn the_crossover_switch_is_saved() {
    use neumann_control_core::settings::CrossoverGuard;
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let guard = || neumann_control_core::settings::load(&path).desk.crossovers;
    open_settings(&mut harness);
    scroll_to_and_click(&mut harness, texts::PROTECT_CROSSOVERS.get());
    harness.run_steps(2);
    assert_eq!(guard(), CrossoverGuard::Open);
    scroll_to_and_click(&mut harness, texts::PROTECT_CROSSOVERS.get());
    harness.run_steps(2);
    assert_eq!(guard(), CrossoverGuard::Protected);
}

/// The delay as the distance sound travels: shown at once on the strip's knob and saved.
#[test]
fn the_delay_unit_switches_to_meters_and_is_saved() {
    let (_simulator, directory, mut harness) =
        connected_speaker_with(SimModel::Kh120Ii, |simulator| {
            simulator.set_value("audio/out/delay", serde_json::json!(480));
        });
    harness.get_by_label("10,00 ms");
    open_settings(&mut harness);
    scroll_to_and_click(&mut harness, texts::DELAY_IN_METERS.get());
    harness.run_steps(2);
    scroll_to_and_click(&mut harness, texts::CLOSE.get());
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::THEME.get()).is_none());
    harness.get_by_label("3,43 m");
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let saved = neumann_control_core::settings::load(&path);
    assert_eq!(
        saved.desk.delay_unit,
        neumann_control_core::settings::DelayUnit::Meters
    );
}

/// The meter range in 10 dB steps, from a list.
#[test]
fn a_meter_floor_between_the_old_steps_is_chosen_and_saved() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_settings(&mut harness);
    harness
        .get_by_role_and_label(Role::ComboBox, texts::METER_RANGE.get())
        .click();
    harness.run_steps(2);
    scroll_to_and_click(&mut harness, &texts::meter_floor(-70.0));
    harness.run_steps(2);
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    let saved = neumann_control_core::settings::load(&path);
    assert_eq!(saved.desk.meter_floor.db(), -70.0);
}

/// "Show master" off hides the master strip on the desk and its row in the compact mode.
#[test]
fn the_master_can_be_hidden() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let master = texts::MASTER_LEVEL.get();
    assert!(harness.query_by_label(master).is_some());
    open_settings(&mut harness);
    scroll_to_and_click(&mut harness, texts::SHOW_MASTER.get());
    harness.run_steps(2);
    scroll_to_and_click(&mut harness, texts::CLOSE.get());
    harness.run_steps(2);
    assert!(harness.query_by_label(master).is_none());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    harness.run_steps(3);
    assert!(harness.query_by_label(master).is_none(), "compact mode");
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    assert!(!neumann_control_core::settings::load(&path).desk.show_master);
}

#[test]
fn english_applies_at_once_and_is_saved() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    assert_eq!(saved.desk.language, LanguageChoice::English);
}

#[test]
fn toolbar_shows_average_response_time_until_turned_off() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
                    .is_some_and(|text| text.contains(','))
            })
            .map(|node| node.rect())
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(&harness).len(), 1);
    let end = turn_knob(&mut harness, DELAY_LABEL, 90.0, 150.0);
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
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
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
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let position = harness
        .get_by_role_and_label(Role::Slider, DELAY_LABEL)
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
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
    harness.get_by_label(texts::METER_INPUT_SHORT.get());
    harness.get_by_label(texts::METER_OUTPUT_SHORT.get());
    // The KH 120 II names its input by the rear switch instead.
    let (_other, _other_directory, kh120) = connected_speaker(SimModel::Kh120Ii);
    kh120.get_by_label(texts::SOURCE_ANALOG.get());
    assert!(kh120
        .query_by_label(texts::METER_OUTPUT_SHORT.get())
        .is_none());
}

#[test]
fn meter_captions_are_centred_under_their_meters() {
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
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
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
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
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
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
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh750);
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
    let (simulator, _directory, analog) = connected_speaker(SimModel::Kh120Ii);
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
    let (_other, _other_directory, kh750) = connected_speaker(SimModel::Kh750);
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
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh120Ii);
    let caption = harness.get_by_label(texts::SOURCE_ANALOG.get()).rect();
    assert!(caption.width() > caption.height(), "{caption:?}");
    let level = harness.get_by_label(&readout(100.0)).rect();
    assert!(
        caption.bottom() <= level.top(),
        "caption {caption:?}, level {level:?}"
    );
}

#[test]
fn the_strip_knob_captions_stay_apart() {
    let (_simulator, _directory, harness) = connected_speaker(SimModel::Kh120Ii);
    let caption = |label: &str| harness.get_by_role_and_label(Role::Label, label).rect();
    let (delay, standby) = (caption(DELAY_LABEL), caption(STANDBY_TIME_LABEL));
    assert!(!delay.intersects(standby), "{delay:?} {standby:?}");
    // One line each: a name too long for its column is cut, never broken inside a word.
    let small = egui::TextStyle::Small.resolve(&harness.ctx.global_style());
    let line = harness.ctx.fonts_mut(|fonts| fonts.row_height(&small));
    for rect in [delay, standby] {
        assert!(rect.height() < 1.5 * line, "{rect:?} for a line of {line}");
    }
}

#[test]
fn each_meter_channel_shows_its_peak_readout() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value("m/in/level", serde_json::json!([-18.2, -40.4]));
    wait_for_label(&mut harness, &texts::meter_readout(Some(-18)));
    harness.get_by_label(&texts::meter_readout(Some(-40)));
    assert_eq!(texts::meter_readout(Some(-18)), "−18");
    assert_eq!(texts::meter_readout(None), "–");
}

#[test]
fn the_language_follows_windows_unless_one_is_chosen() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness_in(path.clone(), Ok(Vec::new()), Language::English);
    harness.run_steps(2);
    harness.get_by_label("Settings");
    drop(harness);
    let mut settings = settings::load(&path);
    settings.desk.language = LanguageChoice::German;
    settings::save(&settings, &path).unwrap();
    let mut harness = harness_in(path, Ok(Vec::new()), Language::English);
    harness.run_steps(2);
    harness.get_by_label("Einstellungen");
}

/// A settings file the app could neither read nor move aside (held open by another program)
/// is not written over with the defaults; the app says so.
#[cfg(windows)]
#[test]
fn an_untouchable_settings_file_is_not_written_over() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let before = std::fs::read(&path).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let mut harness = harness(path.clone(), Vec::new());
    harness.run_steps(2);
    harness.get_by_label(texts::SETTINGS_UNTOUCHABLE.get());
    harness.get_by_label(texts::ADD_GROUP.get()).click();
    harness.run_steps(3);
    drop(held);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// "Detailed log" is kept in the settings; switched off again, it is gone. The log level it sets
/// is process-wide, which tests running side by side share, so `log_detail` tests that part.
#[test]
fn the_detailed_log_is_switched_in_the_settings() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_settings(&mut harness);
    scroll_to_and_click(&mut harness, texts::DETAILED_LOG.get());
    harness.run_steps(2);
    let path = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    assert!(
        neumann_control_core::settings::load(&path)
            .desk
            .detailed_log
    );
    scroll_to_and_click(&mut harness, texts::DETAILED_LOG.get());
    harness.run_steps(2);
    assert!(
        !neumann_control_core::settings::load(&path)
            .desk
            .detailed_log
    );
}
