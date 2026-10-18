//! The compact mode: switching there and back, one row per ungrouped speaker and per group,
//! and rows writing like the strips do.

mod support;

use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::texts;
use neumann_control_core::settings::{self, SETTINGS_FILE};
use neumann_control_core::simulator::SimModel;
use serde_json::Value;
use support::*;

fn press_compact_shortcut(harness: &mut Harness<'static, NeumannApp>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run_steps(2);
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
    harness.set_size(egui::vec2(480.0, 360.0));
    harness.run_steps(2);
    let mute = harness.get_by_label(texts::MUTE.get()).rect();
    let fits = mute.right() <= 480.0;
    let scrolls = harness
        .query_all_by_role(Role::ScrollBar)
        .any(|bar| bar.rect().width() > bar.rect().height());
    assert!(fits || scrolls, "mute at {mute:?} without a way there");
}

#[test]
fn the_toolbar_does_not_overlap_itself_in_the_narrowest_window() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.set_size(egui::vec2(480.0, 360.0));
    harness.run_steps(2);
    let toolbar = [
        texts::SCAN.get(),
        texts::ADD_ADDRESS.get(),
        texts::ADD_GROUP.get(),
        texts::COMPACT.get(),
        texts::SETTINGS.get(),
    ];
    let rects: Vec<egui::Rect> = toolbar
        .iter()
        .map(|label| harness.get_by_label(label).rect())
        .collect();
    for (index, rect) in rects.iter().enumerate() {
        assert!(rect.right() <= 480.0, "{} at {rect:?}", toolbar[index]);
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
    let message = texts::describe_rejection(401);
    wait_until(&mut harness, "the LOCAL message", |harness| {
        harness.query_by_label(&message).is_some()
    });
}
