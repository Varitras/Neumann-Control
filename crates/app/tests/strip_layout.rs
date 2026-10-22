//! A model's strip shows what its stored layout lists; the layout is changed from the strip's and
//! the panel's context menu and in the desk's edit mode.

mod support;

use std::collections::BTreeMap;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::display::shown_value;
use neumann_control_app::paths;
use neumann_control_app::strip_layout;
use neumann_control_app::texts;
use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::settings::{self, StoredStripLayout, StripElement};
use neumann_control_core::simulator::SimModel;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::json;
use support::*;

const MONITOR: &str = "KH 120 II";
const SUBWOOFER: &str = "KH 750";
const INTERFACE: &str = "audio/in/interface";
const STANDBY_SWITCH: &str = "device/standby/enabled";
const TEMPERATURE: &str = "device/temperature";

fn element(text: &str) -> StripElement {
    StripElement::Path(SscPath::parse(text).unwrap())
}

fn layouts(model: &str, above: &[&str], below: &[&str]) -> BTreeMap<String, StoredStripLayout> {
    let layout = StoredStripLayout {
        above: above.iter().map(|text| element(text)).collect(),
        below: below.iter().map(|text| element(text)).collect(),
    };
    BTreeMap::from([(model.to_owned(), layout)])
}

fn entry(text: &str) -> &'static Entry {
    let path = SscPath::parse(text).unwrap();
    Catalog::embedded()
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .expect("a catalog path")
}

fn label(text: &str) -> &'static str {
    entry(text).label(texts::language())
}

#[test]
fn without_a_layout_the_strip_is_as_before() {
    let (_simulators, _directory, harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    for knob in [paths::DELAY, paths::STANDBY_TIME] {
        assert!(
            harness.query_all_by_label(label(knob)).next().is_some(),
            "{knob}"
        );
    }
    for button in [texts::MUTE.get(), texts::IDENTIFY.get(), texts::EQ.get()] {
        harness.get_by_role_and_label(Role::Button, button);
    }
}

#[test]
fn a_stored_layout_shows_on_every_strip_of_its_model() {
    let models = [SimModel::Kh120Ii, SimModel::Kh120Ii, SimModel::Kh750];
    let stored = layouts(MONITOR, &[], &[paths::PHASE_INVERSION]);
    let (_simulators, _directory, harness) = desk_of(&models, stored);
    let phase = label(paths::PHASE_INVERSION);
    assert_eq!(harness.query_all_by_label(phase).count(), 2);
    let mutes = harness.query_all_by_role_and_label(Role::Button, texts::MUTE.get());
    assert_eq!(mutes.count(), 1, "only the KH 750 keeps its mute button");
}

#[test]
fn an_added_select_writes_through_the_policy() {
    let stored = layouts(MONITOR, &[], &[INTERFACE]);
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], stored);
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(2);
    harness.get_by_label("DIGITAL ONLY").click();
    wait_until(&mut harness, "the interface write", |_| {
        sets(&simulators[0], "/audio/in/interface") == [json!("DIGITAL ONLY")]
    });
}

#[test]
fn a_switch_the_model_may_not_write_is_shown_disabled() {
    let stored = layouts(SUBWOOFER, &[], &[STANDBY_SWITCH]);
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh750], stored);
    let button = harness.get_by_role_and_label(Role::Button, label(STANDBY_SWITCH));
    assert!(button.accesskit_node().is_disabled());
    button.click();
    harness.run_steps(3);
    barrier(&simulators[0]);
    assert!(sets(&simulators[0], "/device/standby/enabled").is_empty());
}

#[test]
fn a_display_shows_its_value() {
    let stored = layouts(SUBWOOFER, &[TEMPERATURE], &[]);
    let (simulators, _directory, harness) = desk_of(&[SimModel::Kh750], stored);
    let reported = simulators[0].value(TEMPERATURE).expect("a temperature");
    let shown = shown_value(entry(TEMPERATURE), &reported);
    assert!(harness.query_by_label_contains(&shown).is_some(), "{shown}");
}

fn open_first_panel(harness: &mut Harness<'static, NeumannApp>, name: &str) {
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness.query_all_by_label(name).next().unwrap().click();
    wait_until(harness, "the device panel", |harness| {
        harness.query_by_label(texts::ALL_VALUES.get()).is_some()
    });
    harness.run_steps(1);
}

fn right_click_button(harness: &mut Harness<'static, NeumannApp>, label: &str) {
    harness
        .get_by_role_and_label(Role::Button, label)
        .click_secondary();
    harness.run_steps(2);
}

fn choose(harness: &mut Harness<'static, NeumannApp>, item: &str) {
    harness.get_by_label(item).click();
    harness.run_steps(3);
}

fn stored_layout(directory: &tempfile::TempDir, model: &str) -> Option<StoredStripLayout> {
    let settings = settings::load(&directory.path().join(settings::SETTINGS_FILE));
    settings.desk.strip_layouts.get(model).cloned()
}

#[test]
fn add_to_strip_from_the_panel_reaches_both_monitors() {
    let models = [SimModel::Kh120Ii, SimModel::Kh120Ii, SimModel::Kh750];
    let (_simulators, directory, mut harness) = desk_of(&models, BTreeMap::new());
    open_first_panel(&mut harness, KH120_DEVICE_NAME);
    let phase = label(paths::PHASE_INVERSION);
    harness
        .get_by_role_and_label(Role::CheckBox, phase)
        .click_secondary();
    harness.run_steps(2);
    choose(&mut harness, texts::ADD_TO_STRIP.get());
    let buttons = harness.query_all_by_role_and_label(Role::Button, phase);
    assert_eq!(buttons.count(), 2);
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert!(stored.below.contains(&element(paths::PHASE_INVERSION)));
    assert_eq!(stored_layout(&directory, SUBWOOFER), None);
}

#[test]
fn hide_from_the_strip_menu() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    right_click_button(&mut harness, texts::IDENTIFY.get());
    choose(&mut harness, texts::HIDE.get());
    let identify = harness.query_by_role_and_label(Role::Button, texts::IDENTIFY.get());
    assert!(identify.is_none());
    open_first_panel(&mut harness, KH120_DEVICE_NAME);
    harness
        .get_by_role_and_label(Role::CheckBox, label(paths::IDENTIFY))
        .click_secondary();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::ADD_TO_STRIP.get()).is_some());
    harness
        .get_by_role_and_label(Role::CheckBox, texts::MUTE.get())
        .click_secondary();
    harness.run_steps(2);
    assert!(harness
        .query_by_label(texts::REMOVE_FROM_STRIP.get())
        .is_some());
}

#[test]
fn swap_keeps_the_place() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    right_click_button(&mut harness, texts::IDENTIFY.get());
    choose(&mut harness, texts::SWAP_FOR.get());
    let phase = label(paths::PHASE_INVERSION);
    choose(&mut harness, phase);
    // In reading order: a wide button moves to the next line of the row.
    let place = |label: &str| {
        let rect = harness.get_by_role_and_label(Role::Button, label).rect();
        (rect.top() as i32, rect.left() as i32)
    };
    assert!(place(texts::MUTE.get()) < place(phase));
    assert!(place(phase) < place(texts::EQ.get()));
}

#[test]
fn swap_is_offered_only_with_something_to_add() {
    let catalog = Catalog::embedded();
    let default = strip_layout::resolve(None, SUBWOOFER, catalog);
    let mut everything: Vec<StripElement> = default.above.clone();
    everything.extend(default.below.clone());
    everything.extend(strip_layout::addable(&default, SUBWOOFER, catalog));
    let full = StoredStripLayout {
        above: Vec::new(),
        below: everything,
    };
    let stored = BTreeMap::from([(SUBWOOFER.to_owned(), full)]);
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh750], stored);
    right_click_button(&mut harness, texts::EQ.get());
    let swap = harness.get_by_label(texts::SWAP_FOR.get());
    assert!(swap.accesskit_node().is_disabled());
}

#[test]
fn the_fixed_part_has_no_layout_menu() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    harness.get_by_label(KH120_DEVICE_NAME).click_secondary();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::HIDE.get()).is_none());
    harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .click_secondary();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::HIDE.get()).is_none());
}

#[test]
fn the_menu_writes_nothing() {
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    right_click_button(&mut harness, texts::IDENTIFY.get());
    choose(&mut harness, texts::MOVE_UP.get());
    right_click_button(&mut harness, texts::MUTE.get());
    choose(&mut harness, texts::HIDE.get());
    barrier(&simulators[0]);
    assert!(!any_set(&simulators[0]), "a set reached the speaker");
}

fn start_editing(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_label(texts::CUSTOMIZE_STRIPS.get()).click();
    harness.run_steps(2);
}

fn hide_in_edit_mode(harness: &mut Harness<'static, NeumannApp>, path: &str) {
    harness
        .get_by_label(&texts::hide_element(label(path)))
        .click();
    harness.run_steps(2);
}

fn add_in_edit_mode(harness: &mut Harness<'static, NeumannApp>, offer: &str) {
    harness.get_by_label(texts::ADD_ELEMENT.get()).click();
    harness.run_steps(2);
    choose(harness, offer);
}

const DRAG_FRAMES: usize = 6;

fn drag(harness: &mut Harness<'static, NeumannApp>, from: egui::Pos2, to: egui::Pos2) {
    harness.drag_at(from);
    for frame in 1..=DRAG_FRAMES {
        let progress = frame as f32 / DRAG_FRAMES as f32;
        harness.hover_at(from + (to - from) * progress);
        harness.step();
    }
    harness.drop_at(to);
    harness.run_steps(3);
}

fn center(harness: &Harness<'static, NeumannApp>, role: Role, label: &str) -> egui::Pos2 {
    harness.get_by_role_and_label(role, label).rect().center()
}

fn wheel(harness: &mut Harness<'static, NeumannApp>, notches: f32) {
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, notches),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
}

/// Whether any request carried a value: reads ask with `null` only, `osc` holds the id.
fn any_set(simulator: &neumann_control_core::simulator::Simulator) -> bool {
    fn carries_a_value(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Null => false,
            serde_json::Value::Object(members) => members
                .iter()
                .any(|(key, member)| key != "osc" && carries_a_value(member)),
            _ => true,
        }
    }
    simulator.requests().iter().any(carries_a_value)
}

#[test]
fn editing_hides_and_adds() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    hide_in_edit_mode(&mut harness, paths::IDENTIFY);
    add_in_edit_mode(&mut harness, label(paths::PHASE_INVERSION));
    harness.get_by_label(texts::DONE.get()).click();
    harness.run_steps(2);
    let phase = label(paths::PHASE_INVERSION);
    assert!(harness
        .query_by_role_and_label(Role::Button, phase)
        .is_some());
    let identify = harness.query_by_role_and_label(Role::Button, texts::IDENTIFY.get());
    assert!(identify.is_none());
    assert!(harness.query_by_label(texts::ADD_ELEMENT.get()).is_none());
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert!(stored.below.contains(&element(paths::PHASE_INVERSION)));
    assert!(!stored.below.contains(&element(paths::IDENTIFY)));
}

#[test]
fn editing_sends_nothing() {
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, texts::MUTE.get())
        .click();
    harness.hover_at(center(&harness, Role::Slider, label(paths::DELAY)));
    for _ in 0..4 {
        wheel(&mut harness, 1.0);
        harness.step();
    }
    let fader = center(&harness, Role::Slider, texts::LEVEL.get());
    drag(&mut harness, fader, fader + egui::vec2(0.0, 40.0));
    barrier(&simulators[0]);
    assert!(
        !any_set(&simulators[0]),
        "a set reached the speaker in edit mode"
    );
    harness.get_by_label(texts::DONE.get()).click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::MUTE.get())
        .click();
    wait_until(&mut harness, "the mute after editing", |_| {
        !sets(&simulators[0], "/audio/out/mute").is_empty()
    });
    assert!(any_set(&simulators[0]), "the check itself sees a set");
}

#[test]
fn dragging_moves_across_the_fader() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    let from = center(&harness, Role::Slider, label(paths::DELAY));
    let to = center(&harness, Role::Button, texts::MUTE.get());
    drag(&mut harness, from, to);
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert_eq!(stored.below.first(), Some(&element(paths::DELAY)));
    let knob = harness
        .get_by_role_and_label(Role::Slider, label(paths::DELAY))
        .rect();
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect();
    assert!(knob.top() > fader.bottom());
}

#[test]
fn a_drag_next_to_an_absent_element_keeps_the_stored_order() {
    let above = [paths::DELAY, paths::PHASE_INVERSION, paths::STANDBY_TIME];
    let stored = layouts(MONITOR, &above, &[]);
    let (_simulators, directory, mut harness) =
        desk_prepared(&[SimModel::Kh120Ii], stored, |simulators| {
            simulators[0].remove_value(paths::PHASE_INVERSION);
        });
    start_editing(&mut harness);
    let from = center(&harness, Role::Slider, label(paths::STANDBY_TIME));
    let to = center(&harness, Role::Slider, label(paths::DELAY));
    drag(&mut harness, from, to);
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    let expected = [paths::STANDBY_TIME, paths::DELAY, paths::PHASE_INVERSION];
    assert_eq!(stored.above, expected.map(element).to_vec());
}

#[test]
fn an_unreachable_strip_can_still_be_edited() {
    let (mut simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    simulators.remove(0).stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    start_editing(&mut harness);
    hide_in_edit_mode(&mut harness, paths::IDENTIFY);
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert!(!stored.below.contains(&element(paths::IDENTIFY)));
}

#[test]
fn default_restores_todays_strip() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    hide_in_edit_mode(&mut harness, paths::IDENTIFY);
    harness.get_by_label(texts::DEFAULT_LAYOUT.get()).click();
    harness.run_steps(2);
    let identify = harness.query_by_role_and_label(Role::Button, texts::IDENTIFY.get());
    assert!(identify.is_some());
    assert_eq!(stored_layout(&directory, MONITOR), None);
}

#[test]
fn compact_mode_ends_editing() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    for _ in 0..2 {
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
        harness.run_steps(3);
    }
    assert!(harness.query_by_label(texts::ADD_ELEMENT.get()).is_none());
    let hide = texts::hide_element(label(paths::IDENTIFY));
    assert!(harness.query_by_label(&hide).is_none());
}

#[test]
fn everything_hidden_keeps_the_way_back() {
    let stored = layouts(MONITOR, &[], &[]);
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], stored);
    let mute = texts::MUTE.get();
    assert!(harness
        .query_by_role_and_label(Role::Button, mute)
        .is_none());
    start_editing(&mut harness);
    add_in_edit_mode(&mut harness, label(paths::MUTE));
    assert!(harness
        .query_by_role_and_label(Role::Button, mute)
        .is_some());
}

#[test]
fn the_hint_names_the_model() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    harness.get_by_label(&texts::applies_to_every(MONITOR));
}

#[test]
fn the_button_row_keeps_its_size_and_its_words() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    let buttons = [texts::MUTE.get(), texts::IDENTIFY.get(), texts::EQ.get()];
    let rects = |harness: &Harness<'static, NeumannApp>| -> Vec<egui::Rect> {
        buttons
            .iter()
            .map(|label| harness.get_by_role_and_label(Role::Button, label).rect())
            .collect()
    };
    let first = rects(&harness);
    for _ in 0..6 {
        harness.step();
        assert_eq!(rects(&harness), first, "the buttons moved between frames");
    }
    let one_line = first[0].height();
    for (label, rect) in buttons.iter().zip(&first) {
        assert_eq!(rect.height(), one_line, "{label} wraps its text");
    }
}

#[test]
fn in_edit_mode_no_hide_mark_covers_a_knob() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    let knobs = [paths::DELAY, paths::STANDBY_TIME];
    for own in knobs {
        let mark = harness
            .get_by_label(&texts::hide_element(label(own)))
            .rect();
        for knob_path in knobs {
            let knob = harness
                .get_by_role_and_label(Role::Slider, label(knob_path))
                .rect();
            assert!(!mark.intersects(knob), "the × of {own} lies on {knob_path}");
        }
    }
}
