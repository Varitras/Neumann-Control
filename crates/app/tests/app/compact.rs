//! The compact mode: switching there and back, one row per ungrouped speaker and per group,
//! rows writing like the strips do, and each model's buttons chosen by right click.

use crate::support::*;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::paths;
use neumann_control_app::texts;
use neumann_control_app::window_placement::WINDOW_MIN_SIZE;
use neumann_control_core::settings::{self, CompactButton, SETTINGS_FILE};
use neumann_control_core::simulator::SimModel;
use neumann_control_core::ssc::wire::DeviceError;
use serde_json::Value;
use std::collections::BTreeMap;

fn press_compact_shortcut(harness: &mut Harness<'static, NeumannApp>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run_steps(2);
}

const MONITOR: &str = "KH 120 II";
const SUBWOOFER: &str = "KH 750";

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

fn stored_buttons(directory: &tempfile::TempDir, model: &str) -> Option<Vec<CompactButton>> {
    let settings = settings::load(&directory.path().join(SETTINGS_FILE));
    settings.desk.compact_layouts.get(model).cloned()
}

fn switch(text: &str) -> CompactButton {
    CompactButton::Switch(paths::path(text))
}

fn left_of(harness: &Harness<'static, NeumannApp>, label: &str) -> f32 {
    harness
        .get_by_role_and_label(Role::Button, label)
        .rect()
        .left()
}

fn drag_level(harness: &mut Harness<'static, NeumannApp>, label: &str, by: f32) {
    let start = harness
        .get_by_role_and_label(Role::Slider, label)
        .rect()
        .center();
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(by * frame as f32 / 4.0, 0.0));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(by, 0.0));
}

#[test]
fn the_shortcut_switches_to_compact_and_back_and_is_saved() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let path = directory.path().join(SETTINGS_FILE);
    harness.get_by_label(texts::ADD_GROUP.get());
    press_compact_shortcut(&mut harness);
    assert!(settings::load(&path).desk.compact);
    assert!(
        harness.query_by_label(texts::ADD_GROUP.get()).is_none(),
        "the compact toolbar leaves adding to the desk"
    );
    harness.get_by_label(KH120_DEVICE_NAME);
    harness.get_by_label(texts::MASTER.get());
    harness.get_by_label(texts::COMPACT.get()).click();
    harness.run_steps(2);
    assert!(!settings::load(&path).desk.compact);
    harness.get_by_label(texts::ADD_GROUP.get());
}

#[test]
fn a_compact_row_writes_its_level_once_when_let_go() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press_compact_shortcut(&mut harness);
    drag_level(&mut harness, texts::LEVEL.get(), -40.0);
    wait_until(&mut harness, "a level set", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(5);
    barrier(&simulator);
    let sets = level_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0] < 100.0, "{sets:?}");
}

#[test]
fn a_group_has_one_row_instead_of_its_members() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    press_compact_shortcut(&mut harness);
    assert!(harness.query_by_label(KH120_DEVICE_NAME).is_none());
    assert!(harness.query_by_label(KH750_DEVICE_NAME).is_none());
    harness.get_by_label(GROUP_NAME);
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "group mute", |_| {
        sets(&kh120, "/audio/out/mute") == [Value::Bool(true)]
            && sets(&kh750, "/audio/out/mute") == [Value::Bool(true)]
    });
    drag_level(&mut harness, &texts::group_level(GROUP_NAME), -20.0);
    wait_until(&mut harness, "both level sets", |_| {
        !sets(&kh120, "/audio/out/level").is_empty() && !sets(&kh750, "/audio/out/level").is_empty()
    });
    assert_eq!(
        sets(&kh120, "/audio/out/level"),
        sets(&kh750, "/audio/out/level")
    );
}

#[test]
fn compact_rows_are_one_line_high_and_follow_each_other() {
    let (_kh120, _kh750, _directory, mut harness) = grouped(Vec::new());
    press_compact_shortcut(&mut harness);
    let center = |harness: &Harness<'static, NeumannApp>, label: &str| {
        harness
            .get_by_role_and_label(Role::Slider, label)
            .rect()
            .center()
            .y
    };
    let group = center(&harness, &texts::group_level(GROUP_NAME));
    let master = center(&harness, texts::MASTER_LEVEL.get());
    // Neighbouring rows: their faders are about one row apart, not a window height.
    assert!(master - group < 60.0, "group {group}, master {master}");
}

#[test]
fn compact_columns_line_up_between_speaker_and_master_rows() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press_compact_shortcut(&mut harness);
    let bars: Vec<f32> = harness
        .query_all_by_label(texts::INPUT_METERS.get())
        .map(|node| node.rect().left())
        .collect();
    assert_eq!(bars.len(), 2, "speaker and master row");
    assert!((bars[0] - bars[1]).abs() < 0.5, "{bars:?}");
}

#[test]
fn mute_stays_reachable_in_the_narrowest_compact_window() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press_compact_shortcut(&mut harness);
    harness.set_size(WINDOW_MIN_SIZE.into());
    harness.run_steps(2);
    let mute = harness.get_by_label(texts::MUTE.get()).rect();
    let fits = mute.right() <= WINDOW_MIN_SIZE[0];
    let scrolls = harness
        .query_all_by_role(Role::ScrollBar)
        .any(|bar| bar.rect().width() > bar.rect().height());
    assert!(fits || scrolls, "mute at {mute:?} without a way there");
}

#[test]
fn the_toolbar_does_not_overlap_itself_in_the_narrowest_window() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.set_size(WINDOW_MIN_SIZE.into());
    // The widest toolbar: with the average response time, which comes after a measurement.
    wait_until(&mut harness, "the average response time", |harness| {
        harness.query_by_label_contains("Ø ").is_some()
    });
    harness.run_steps(2);
    let toolbar = [
        texts::SCAN.get(),
        texts::ADD_ADDRESS.get(),
        texts::ADD_GROUP.get(),
        texts::CUSTOMIZE_STRIPS.get(),
        texts::COMPACT.get(),
        texts::SETTINGS.get(),
        texts::HELP.get(),
    ];
    let rects: Vec<egui::Rect> = toolbar
        .iter()
        .map(|label| harness.get_by_label(label).rect())
        .collect();
    for (index, rect) in rects.iter().enumerate() {
        assert!(
            rect.right() <= WINDOW_MIN_SIZE[0],
            "{} at {rect:?}",
            toolbar[index]
        );
        for other in &rects[index + 1..] {
            assert!(!rect.intersects(*other), "{rect:?} and {other:?}");
        }
    }
    if let Some(count) = harness.query_by_label(&texts::speaker_count(1)) {
        let count = count.rect();
        assert!(
            rects.iter().all(|rect| !rect.intersects(count)),
            "{count:?}"
        );
    }
}

#[test]
fn a_compact_row_says_why_a_write_was_refused() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value("ui/control_mode", serde_json::json!("LOCAL"));
    press_compact_shortcut(&mut harness);
    drag_level(&mut harness, texts::LEVEL.get(), -40.0);
    let message = texts::describe_rejection(DeviceError::LOCAL_MODE);
    wait_until(&mut harness, "the LOCAL message", |harness| {
        harness.query_by_label(&message).is_some()
    });
}

#[test]
fn a_group_row_says_why_a_members_write_was_refused() {
    let (kh120, _kh750, _directory, mut harness) = grouped(Vec::new());
    kh120.set_value("ui/control_mode", serde_json::json!("LOCAL"));
    press_compact_shortcut(&mut harness);
    drag_level(&mut harness, &texts::group_level(GROUP_NAME), -40.0);
    let message = texts::describe_rejection(DeviceError::LOCAL_MODE);
    // On the group row and on the master row, which both hold the member.
    wait_until(&mut harness, "the LOCAL message", |harness| {
        harness.query_all_by_label_contains(&message).count() == 2
    });
}

/// The default buttons after mute; the EQ window, drawn inside the test window, would lie over
/// the row, so each is opened by a test of its own.
#[test]
fn a_compact_row_opens_the_eq() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press_compact_shortcut(&mut harness);
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::EQ.get())
        .click();
    wait_for_label(&mut harness, texts::EQ_COMPUTED.get());
}

#[test]
fn a_compact_row_opens_the_details() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press_compact_shortcut(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, texts::DETAILS.get())
        .click();
    wait_until(&mut harness, "the device panel", |harness| {
        harness.query_by_label(texts::TAB_OVERVIEW.get()).is_some()
    });
}

#[test]
fn a_compact_button_is_hidden_and_another_added_by_right_click() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    press_compact_shortcut(&mut harness);
    right_click_button(&mut harness, texts::MUTE.get());
    let menus = harness.query_all_by_label(texts::ADD_BUTTON.get()).count();
    assert_eq!(menus, 1, "the button's menu only, not also the row's");
    choose(&mut harness, texts::HIDE.get());
    assert!(harness
        .query_by_role_and_label(Role::Button, texts::MUTE.get())
        .is_none());
    let hidden = vec![CompactButton::Eq, CompactButton::Details];
    assert_eq!(stored_buttons(&directory, MONITOR), Some(hidden));
    right_click_button(&mut harness, texts::EQ.get());
    choose(&mut harness, texts::ADD_BUTTON.get());
    // The menu names it as the button does.
    choose(&mut harness, texts::IDENTIFY.get());
    harness.get_by_role_and_label(Role::Button, texts::IDENTIFY.get());
    let stored = stored_buttons(&directory, MONITOR).unwrap();
    assert_eq!(stored.last(), Some(&switch(paths::IDENTIFY)));
}

#[test]
fn a_compact_button_moves_right_and_the_default_comes_back() {
    let (_simulators, directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    press_compact_shortcut(&mut harness);
    right_click_button(&mut harness, texts::MUTE.get());
    choose(&mut harness, texts::MOVE_RIGHT.get());
    assert!(left_of(&harness, texts::EQ.get()) < left_of(&harness, texts::MUTE.get()));
    right_click_button(&mut harness, texts::MUTE.get());
    choose(&mut harness, texts::DEFAULT_LAYOUT.get());
    assert!(left_of(&harness, texts::MUTE.get()) < left_of(&harness, texts::EQ.get()));
    assert_eq!(stored_buttons(&directory, MONITOR), None);
}

/// With no button left the rest of the row still offers adding one.
#[test]
fn a_row_without_buttons_still_offers_adding() {
    let (_simulators, _directory, mut harness) = desk_of(&[SimModel::Kh120Ii], BTreeMap::new());
    press_compact_shortcut(&mut harness);
    for button in [texts::MUTE, texts::EQ, texts::DETAILS] {
        right_click_button(&mut harness, button.get());
        choose(&mut harness, texts::HIDE.get());
    }
    harness.get_by_label(KH120_DEVICE_NAME).click_secondary();
    harness.run_steps(2);
    choose(&mut harness, texts::ADD_BUTTON.get());
    choose(&mut harness, texts::DETAILS.get());
    harness.get_by_role_and_label(Role::Button, texts::DETAILS.get());
}

#[test]
fn compact_buttons_are_chosen_per_model() {
    let models = [SimModel::Kh120Ii, SimModel::Kh750];
    let (_simulators, directory, mut harness) = desk_of(&models, BTreeMap::new());
    press_compact_shortcut(&mut harness);
    let row_of = |harness: &Harness<'static, NeumannApp>, name: &str| {
        harness.get_by_label(name).rect().center().y
    };
    let monitor_row = row_of(&harness, KH120_DEVICE_NAME);
    let monitor_eq = harness
        .query_all_by_role_and_label(Role::Button, texts::EQ.get())
        .find(|node| (node.rect().center().y - monitor_row).abs() < 10.0)
        .expect("the monitor's EQ button");
    monitor_eq.click_secondary();
    harness.run_steps(2);
    choose(&mut harness, texts::HIDE.get());
    let left: Vec<f32> = harness
        .query_all_by_role_and_label(Role::Button, texts::EQ.get())
        .map(|node| node.rect().center().y)
        .collect();
    let subwoofer_row = row_of(&harness, KH750_DEVICE_NAME);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!((left[0] - subwoofer_row).abs() < 10.0, "{left:?}");
    assert!(stored_buttons(&directory, MONITOR).is_some());
    assert_eq!(stored_buttons(&directory, SUBWOOFER), None);
}

/// As on the desk, a right click on a group's compact row offers to delete it; the dialog asks.
#[test]
fn a_right_click_on_a_compact_group_row_deletes_the_group() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    press_compact_shortcut(&mut harness);
    harness.get_by_label(GROUP_NAME).click_secondary();
    harness.run_steps(2);
    harness.get_by_label(texts::DELETE_GROUP_MENU.get()).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::delete_question(GROUP_NAME));
    harness.get_by_label(texts::DELETE_GROUP.get()).click();
    harness.run_steps(2);
    assert!(settings::load(&directory.path().join(SETTINGS_FILE))
        .groups
        .is_empty());
}
