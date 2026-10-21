//! A model's strip shows what its stored layout lists; the layout is changed from the strip's and
//! the panel's context menu and in the desk's edit mode.

mod support;

use std::collections::BTreeMap;

use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use neumann_control_app::display::shown_value;
use neumann_control_app::paths;
use neumann_control_app::texts;
use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::settings::{StoredStripLayout, StripElement};
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
        assert!(harness.query_all_by_label(label(knob)).next().is_some(), "{knob}");
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
