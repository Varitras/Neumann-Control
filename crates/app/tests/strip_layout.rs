//! A model's strip shows what its stored layout lists; the layout is changed from the strip's and
//! the panel's context menu and in the desk's edit mode.

mod support;

use std::collections::BTreeMap;

use eframe::egui::accesskit::Role;
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
    let left = |label: &str| {
        harness
            .get_by_role_and_label(Role::Button, label)
            .rect()
            .left()
    };
    assert!(left(texts::MUTE.get()) < left(phase));
    assert!(left(phase) < left(texts::EQ.get()));
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
    let written = simulators[0].requests().iter().any(|request| {
        request.to_string().contains("true") || request.to_string().contains("false")
    });
    assert!(!written, "a set reached the speaker");
}
