//! A model's strip shows what its stored layout lists; the layout is changed from the strip's and
//! the panel's context menu and in the desk's edit mode.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::support::*;
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

const MONITOR: &str = "KH 120 II";
const SUBWOOFER: &str = "KH 750";

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
fn without_a_layout_the_strip_shows_the_working_values() {
    let (_simulators, _directory, harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    let shown = [
        paths::INPUT_INTERFACE,
        paths::CURRENT_INPUT,
        paths::STANDBY_COUNTDOWN,
        paths::STANDBY_ENABLED,
    ];
    for path in shown {
        assert!(
            harness.query_all_by_label(label(path)).next().is_some(),
            "{path}"
        );
    }
    for set_once in [paths::DELAY, paths::STANDBY_TIME, paths::LOGO_BRIGHTNESS] {
        assert!(
            harness.query_all_by_label(label(set_once)).next().is_none(),
            "{set_once}"
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
    let stored = layouts(MONITOR, &[], &[paths::INPUT_INTERFACE]);
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
    let stored = layouts(SUBWOOFER, &[], &[paths::STANDBY_ENABLED]);
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh750], stored);
    let button = harness.get_by_role_and_label(Role::Button, label(paths::STANDBY_ENABLED));
    assert!(button.accesskit_node().is_disabled());
    button.click();
    harness.run_steps(3);
    barrier(&simulators[0]);
    assert!(sets(&simulators[0], "/device/standby/enabled").is_empty());
}

#[test]
fn a_display_shows_its_value() {
    let stored = layouts(SUBWOOFER, &[paths::TEMPERATURE], &[]);
    let (simulators, _directory, harness) = desk_of(&[SimModel::Kh750], stored);
    let reported = simulators[0]
        .value(paths::TEMPERATURE)
        .expect("a temperature");
    let shown = shown_value(entry(paths::TEMPERATURE), &reported);
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

/// Edit mode has no × any more: an element is hidden from its context menu. `shown` is the
/// element's text on the strip ("Ident." for identify).
fn hide_in_edit_mode(harness: &mut Harness<'static, NeumannApp>, shown: &str) {
    harness.get_by_label(shown).click_secondary();
    harness.run_steps(2);
    choose(harness, texts::HIDE.get());
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
    hide_in_edit_mode(&mut harness, texts::IDENTIFY.get());
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
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], knob_strip_layouts());
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

/// While a knob is dragged over the buttons, the strip already shows it there: the knob row
/// above the fader closes and the buttons below make room. Nothing is stored before letting go.
#[test]
fn a_drag_shows_where_the_element_lands_before_letting_go() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], knob_strip_layouts());
    start_editing(&mut harness);
    let mute_before = center(&harness, Role::Button, texts::MUTE.get());
    let from = center(&harness, Role::Slider, label(paths::DELAY));
    harness.drag_at(from);
    for frame in 1..=DRAG_FRAMES {
        let progress = frame as f32 / DRAG_FRAMES as f32;
        harness.hover_at(from + (mute_before - from) * progress);
        harness.step();
    }
    harness.run_steps(2);
    let delay = center(&harness, Role::Slider, label(paths::DELAY));
    // The landing is drawn from where the strip drew its elements the frame before; it must
    // settle, not flicker between two places.
    for _ in 0..4 {
        harness.step();
        assert_eq!(center(&harness, Role::Slider, label(paths::DELAY)), delay);
    }
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect();
    assert!(
        delay.y > fader.bottom(),
        "the delay knob is shown below the fader"
    );
    let mute_now = center(&harness, Role::Button, texts::MUTE.get());
    assert_ne!(mute_now, mute_before, "mute made room");
    assert_eq!(
        stored_layout(&directory, MONITOR),
        knob_strip_layouts().remove(MONITOR),
        "nothing stored while dragging"
    );
    harness.drop_at(mute_before);
    harness.run_steps(3);
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert_eq!(stored.below.first(), Some(&element(paths::DELAY)));
}

/// Layouts are per model: a knob of a KH 120 II let go on a KH 750 strip lands nowhere.
#[test]
fn a_drag_onto_another_models_strip_changes_nothing() {
    let (_simulators, directory, mut harness) =
        desk_of(&[SimModel::Kh120Ii, SimModel::Kh750], knob_strip_layouts());
    start_editing(&mut harness);
    let delays: Vec<egui::Pos2> = harness
        .query_all_by_role_and_label(Role::Slider, label(paths::DELAY))
        .map(|node| node.rect().center())
        .collect();
    let (from, other_strip) = (delays[0], delays[1]);
    let other_mutes: Vec<egui::Pos2> = harness
        .query_all_by_role_and_label(Role::Button, texts::MUTE.get())
        .map(|node| node.rect().center())
        .collect();
    drag(&mut harness, from, other_mutes[1]);
    assert!(other_strip.x > from.x, "the KH 750 strip is the second one");
    let layouts = knob_strip_layouts();
    for model in [MONITOR, SUBWOOFER] {
        assert_eq!(
            stored_layout(&directory, model),
            layouts.get(model).cloned(),
            "{model}"
        );
    }
}

#[test]
fn dragging_moves_across_the_fader() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], knob_strip_layouts());
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
    hide_in_edit_mode(&mut harness, texts::IDENTIFY.get());
    let stored = stored_layout(&directory, MONITOR).expect("a stored layout");
    assert!(!stored.below.contains(&element(paths::IDENTIFY)));
}

#[test]
fn default_restores_todays_strip() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    start_editing(&mut harness);
    hide_in_edit_mode(&mut harness, texts::IDENTIFY.get());
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

/// A right click opens the element's menu; with the button held and moved it does not drag the
/// element along (it used to jump to the pointer).
#[test]
fn a_right_drag_in_edit_mode_moves_nothing() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], knob_strip_layouts());
    start_editing(&mut harness);
    let from = center(&harness, Role::Slider, label(paths::DELAY));
    let to = center(&harness, Role::Button, texts::MUTE.get());
    harness.hover_at(from);
    let press = |pressed, pos| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.event(press(true, from));
    harness.step();
    for frame in 1..=DRAG_FRAMES {
        let progress = frame as f32 / DRAG_FRAMES as f32;
        harness.hover_at(from + (to - from) * progress);
        harness.step();
        assert_eq!(
            center(&harness, Role::Slider, label(paths::DELAY)),
            from,
            "the knob followed the right button"
        );
    }
    harness.event(press(false, to));
    harness.run_steps(3);
    let stored = stored_layout(&directory, MONITOR).expect("the test layout");
    assert_eq!(stored.above.first(), Some(&element(paths::DELAY)));
}

/// The input meters' places while the levels change, in normal and in edit mode.
fn meters_while_levels_change(editing: bool) -> Vec<Vec<egui::Rect>> {
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    if editing {
        start_editing(&mut harness);
    }
    let mut places = Vec::new();
    for level in [-119.0, -60.0, -6.0, -100.0, -0.5, -45.0] {
        simulators[0].set_value("m/in/level", json!([level, level - 3.0]));
        harness.run_steps(4);
        let meters: Vec<egui::Rect> = harness
            .query_all_by_role(Role::ProgressIndicator)
            .map(|node| node.rect())
            .collect();
        places.push(meters);
    }
    places
}

#[test]
fn the_meters_stay_in_place_while_the_levels_change() {
    for editing in [false, true] {
        let places = meters_while_levels_change(editing);
        assert!(!places[0].is_empty());
        for later in &places[1..] {
            assert_eq!(*later, places[0], "editing {editing}");
        }
    }
}

/// Peak readouts wider than "−120" in the app's proportional font.
const WIDE_READOUTS: [i32; 6] = [-109, -108, -106, -104, -103, -100];

/// A wide peak readout must not widen its column and push the meters beside it.
#[test]
fn no_meter_moves_when_the_peak_readout_changes() {
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh750], BTreeMap::new());
    let places = |harness: &Harness<'static, NeumannApp>| -> Vec<egui::Rect> {
        harness
            .query_all_by_role(Role::ProgressIndicator)
            .map(|node| node.rect())
            .collect()
    };
    simulators[0].set_value("m/in/level", json!([-119.0, -119.0]));
    harness.run_steps(8);
    let first = places(&harness);
    assert!(!first.is_empty());
    // Rising, so the held peak and with it the readout takes each level at once.
    for readout in WIDE_READOUTS {
        let level = f64::from(readout);
        simulators[0].set_value("m/in/level", json!([level, level]));
        let shown = texts::meter_readout(Some(readout));
        wait_until(&mut harness, &shown, |harness| {
            harness.query_all_by_label_contains(&shown).next().is_some()
        });
        assert_eq!(places(&harness), first, "readout {shown}");
    }
}

/// A falling meter asks for its next frame after a pause, not at once: at the monitor's own
/// rate the window kept the GPU busy all the time.
#[test]
fn a_falling_meter_does_not_repaint_at_once() {
    let (simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    simulators[0].set_value("m/in/level", json!([-6.0, -6.0]));
    let high = texts::meter_readout(Some(-6));
    wait_until(&mut harness, &high, |harness| {
        harness.query_all_by_label_contains(&high).next().is_some()
    });
    simulators[0].set_value("m/in/level", json!([-119.0, -119.0]));
    // A new reading wakes the window at once; the frames of the fall after it must not.
    wait_until(&mut harness, "a frame booked after a pause", |harness| {
        let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        !delay.is_zero() && delay < Duration::MAX
    });
}

#[test]
fn no_strip_moves_while_values_change() {
    let models = [
        SimModel::Kh120Ii,
        SimModel::Kh750,
        SimModel::Kh750,
        SimModel::Kh120Ii,
    ];
    let (simulators, _directory, mut harness) = desk_of(&models, BTreeMap::new());
    let places = |harness: &Harness<'static, NeumannApp>| -> Vec<egui::Rect> {
        let names = [KH120_DEVICE_NAME, KH750_DEVICE_NAME]
            .into_iter()
            .flat_map(|name| harness.query_all_by_label(name).map(|node| node.rect()));
        let faders = harness
            .query_all_by_role_and_label(Role::Slider, texts::LEVEL.get())
            .map(|node| node.rect());
        names.chain(faders).collect()
    };
    let first = places(&harness);
    for (round, level) in [-119.0, -6.0, -60.0, -0.5, -100.0, -20.0]
        .into_iter()
        .enumerate()
    {
        for simulator in &simulators {
            simulator.set_value("m/in/level", json!([level, level - 1.0]));
            simulator.set_value("m/out/level", json!([level, level, level]));
        }
        harness.run_steps(4);
        assert_eq!(places(&harness), first, "round {round}");
    }
}

#[test]
fn no_strip_moves_in_a_window_narrower_than_the_desk() {
    let models = [
        SimModel::Kh120Ii,
        SimModel::Kh750,
        SimModel::Kh750,
        SimModel::Kh120Ii,
    ];
    let (_simulators, _directory, mut harness) = desk_of(&models, BTreeMap::new());
    for (width, height) in [
        (700.0, 600.0),
        (900.0, 700.0),
        (1100.0, 500.0),
        (600.0, 900.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.run_steps(4);
        let places = |harness: &Harness<'static, NeumannApp>| -> Vec<egui::Rect> {
            harness
                .query_all_by_role_and_label(Role::Slider, texts::LEVEL.get())
                .map(|node| node.rect())
                .collect()
        };
        let first = places(&harness);
        for frame in 0..8 {
            harness.step();
            assert_eq!(places(&harness), first, "{width}x{height}, frame {frame}");
        }
    }
}

/// With "Same layout for every speaker of a model" off, hiding an element on one speaker's
/// strip leaves the other speaker of that model as it was.
#[test]
fn own_layouts_keep_the_other_speaker_of_the_model() {
    let models = [SimModel::Kh120Ii, SimModel::Kh120Ii];
    let (_simulators, directory, mut harness) = desk_of(&models, BTreeMap::new());
    switch_to_own_layouts(&mut harness);
    let identify = |harness: &Harness<'static, NeumannApp>| {
        harness
            .query_all_by_role_and_label(Role::Button, texts::IDENTIFY.get())
            .count()
    };
    assert_eq!(identify(&harness), 2);
    harness
        .query_all_by_role_and_label(Role::Button, texts::IDENTIFY.get())
        .next()
        .unwrap()
        .click_secondary();
    harness.run_steps(2);
    choose(&mut harness, texts::HIDE.get());
    assert_eq!(identify(&harness), 1);
    let settings = settings::load(&directory.path().join(settings::SETTINGS_FILE));
    assert!(settings.desk.strip_layouts.is_empty());
    assert_eq!(settings.desk.speaker_strip_layouts.len(), 1);
}

/// Switches "Same layout for every speaker of a model" off in the settings.
fn switch_to_own_layouts(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::LAYOUTS_PER_MODEL.get()).click();
    harness.run_steps(1);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
}

/// With own layouts, a knob let go on the other speaker's strip of the same model lands nowhere:
/// it belongs to its own speaker's layout.
#[test]
fn with_own_layouts_a_drag_onto_another_speakers_strip_changes_nothing() {
    let models = [SimModel::Kh120Ii, SimModel::Kh120Ii];
    let (_simulators, directory, mut harness) = desk_of(&models, knob_strip_layouts());
    switch_to_own_layouts(&mut harness);
    start_editing(&mut harness);
    let delays: Vec<egui::Pos2> = harness
        .query_all_by_role_and_label(Role::Slider, label(paths::DELAY))
        .map(|node| node.rect().center())
        .collect();
    let other_mutes: Vec<egui::Pos2> = harness
        .query_all_by_role_and_label(Role::Button, texts::MUTE.get())
        .map(|node| node.rect().center())
        .collect();
    assert!(other_mutes[1].x > delays[0].x, "the second strip is to the right");
    drag(&mut harness, delays[0], other_mutes[1]);
    let settings = settings::load(&directory.path().join(settings::SETTINGS_FILE));
    assert!(settings.desk.speaker_strip_layouts.is_empty());
}
