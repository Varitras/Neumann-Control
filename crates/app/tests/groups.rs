//! Groups on the desk: group fader, group and master mute, linked settings, the group dialog
//! and choosing a group in the device panel.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_core::settings::{self, GroupSettings, Linkable, Settings, SETTINGS_FILE};
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::Value;
use support::*;

const KH120_ID: &str = "serial:SIM0001234";
const KH750_ID: &str = "serial:SIM0007500";
const GROUP_NAME: &str = "Front";

/// A KH 120 II and a KH 750 in one group "Front", linking `linked`, both connected.
fn grouped(
    linked: Vec<Linkable>,
) -> (
    Simulator,
    Simulator,
    tempfile::TempDir,
    Harness<'static, NeumannApp>,
) {
    let kh120 = Simulator::start(SimModel::Kh120Ii);
    let kh750 = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let mut first = manual_for(&kh120);
    first.serial = Some("SIM0001234".to_owned());
    let mut second = manual_for(&kh750);
    second.serial = Some("SIM0007500".to_owned());
    let settings = Settings {
        manual: vec![first, second],
        groups: vec![GroupSettings {
            name: GROUP_NAME.to_owned(),
            color: 0,
            members: vec![KH120_ID.to_owned(), KH750_ID.to_owned()],
            linked,
        }],
        ..Settings::default()
    };
    settings::save(&settings, &path).unwrap();
    let mut harness = harness(path, Vec::new());
    harness.set_size(egui::vec2(1200.0, 900.0));
    wait_until(&mut harness, "both strips connected", |harness| {
        harness.query_by_label(KH120_DEVICE_NAME).is_some()
            && harness.query_by_label(KH750_DEVICE_NAME).is_some()
            && harness.query_all_by_label(&readout(100.0)).count() == 2
    });
    wait_for_initial_read(&mut harness, &kh120);
    wait_for_initial_read(&mut harness, &kh750);
    (kh120, kh750, directory, harness)
}

fn sets(simulator: &Simulator, pointer: &str) -> Vec<Value> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| request.pointer(pointer).filter(|value| !value.is_null()))
        .cloned()
        .collect()
}

#[test]
fn group_fader_moves_every_member_by_the_same_amount() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    let label = texts::group_level(GROUP_NAME);
    let start = harness
        .get_by_role_and_label(Role::Slider, &label)
        .rect()
        .center();
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "both level sets", |_| {
        !sets(&kh120, "/audio/out/level").is_empty() && !sets(&kh750, "/audio/out/level").is_empty()
    });
    let first = sets(&kh120, "/audio/out/level");
    let second = sets(&kh750, "/audio/out/level");
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first, second);
    assert!(first[0].as_f64().unwrap() < 100.0);
}

#[test]
fn linked_mute_reaches_the_other_member() {
    let (kh120, kh750, _directory, mut harness) = grouped(vec![Linkable::Mute]);
    harness
        .get_all_by_label(texts::MUTE)
        .next()
        .unwrap()
        .click();
    wait_until(&mut harness, "both mute sets", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty() && !sets(&kh750, "/audio/out/mute").is_empty()
    });
}

#[test]
fn unlinked_mute_stays_with_its_speaker() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    harness
        .get_all_by_label(texts::MUTE)
        .next()
        .unwrap()
        .click();
    wait_until(&mut harness, "one mute set", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty() || !sets(&kh750, "/audio/out/mute").is_empty()
    });
    harness.run_steps(5);
    barrier(&kh120);
    barrier(&kh750);
    let total = sets(&kh120, "/audio/out/mute").len() + sets(&kh750, "/audio/out/mute").len();
    assert_eq!(total, 1);
}

#[test]
fn group_and_master_mute_switch_all() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "group mute", |_| {
        sets(&kh120, "/audio/out/mute") == [Value::Bool(true)]
            && sets(&kh750, "/audio/out/mute") == [Value::Bool(true)]
    });
    // All muted now, so the master switches all back on.
    harness.get_by_label(texts::MUTE_ALL).click();
    let unmuted = [Value::Bool(true), Value::Bool(false)];
    wait_until(&mut harness, "master unmute", |_| {
        sets(&kh120, "/audio/out/mute") == unmuted && sets(&kh750, "/audio/out/mute") == unmuted
    });
}

#[test]
fn link_buttons_toggle_and_are_saved() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    harness
        .get_by_label(&texts::link_label(texts::LINK_LEVEL))
        .click();
    harness.run_steps(2);
    let saved = settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(saved.groups[0].linked, vec![Linkable::Level]);
}

#[test]
fn group_dialog_renames_and_deletes() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    harness.get_by_label(GROUP_NAME).click();
    wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Hinten");
    harness.get_by_label(texts::CLOSE).click();
    harness.run_steps(2);
    let path = directory.path().join(SETTINGS_FILE);
    assert_eq!(settings::load(&path).groups[0].name, "Hinten");
    harness.get_by_label("Hinten").click();
    wait_for_label(&mut harness, texts::DELETE_GROUP);
    harness.get_by_label(texts::DELETE_GROUP).click();
    harness.run_steps(2);
    assert!(settings::load(&path).groups.is_empty());
}

#[test]
fn add_group_creates_one_and_the_panel_assigns_a_speaker() {
    let (simulator, directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::ADD_GROUP).click();
    wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE);
    harness.get_by_label(texts::CLOSE).click();
    harness.run_steps(2);
    let path = directory.path().join(SETTINGS_FILE);
    assert_eq!(settings::load(&path).groups.len(), 1);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness
        .get_by_role_and_label(Role::ComboBox, texts::GROUP)
        .click();
    harness.run_steps(2);
    let name = settings::load(&path).groups[0].name.clone();
    harness.get_all_by_label(&name).last().unwrap().click();
    harness.run_steps(2);
    let saved = settings::load(&path);
    assert_eq!(saved.groups[0].members.len(), 1, "{:?}", saved.groups);
    drop(simulator);
}

#[test]
fn dragging_a_strip_onto_another_puts_it_in_front_and_saves_the_order() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    let handles: Vec<egui::Rect> = harness
        .query_all_by_label(texts::MOVE_STRIP)
        .map(|node| node.rect())
        .collect();
    assert_eq!(handles.len(), 2);
    let (first, second) = (handles[0], handles[1]);
    harness.drag_at(second.center());
    for frame in 1..=6 {
        let progress = frame as f32 / 6.0;
        harness.hover_at(second.center() + (first.center() - second.center()) * progress);
        harness.step();
    }
    harness.drop_at(first.center());
    harness.run_steps(2);
    let saved = settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(saved.order, vec![KH750_ID.to_owned(), KH120_ID.to_owned()]);
}
