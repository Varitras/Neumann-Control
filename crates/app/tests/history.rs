//! Every write lands in the session's history as one entry per user action, and each entry can
//! be taken back: the values before are written again, through the same rules as any write.

mod support;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::history::{is_open, Entry};
use neumann_control_app::texts;
use neumann_control_core::simulator::SimModel;
use serde_json::{json, Value};
use support::*;

const LEVEL_PATH: &str = "audio/out/level";

fn entries(harness: &Harness<'static, NeumannApp>) -> Vec<Entry> {
    harness.state().history().entries().cloned().collect()
}

fn drag_fader_down(harness: &mut Harness<'static, NeumannApp>) {
    let start = harness
        .get_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .rect()
        .center();
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    harness.run_steps(3);
}

fn take_back(harness: &mut Harness<'static, NeumannApp>, id: u64) {
    harness.state_mut().request_take_back(id);
    harness.run_steps(3);
}

#[test]
fn a_fader_drag_is_one_history_entry() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value(LEVEL_PATH, json!(90.0));
    wait_for_level(&mut harness, 90.0);
    drag_fader_down(&mut harness);
    wait_until(&mut harness, "the level write", |_| {
        !level_sets(&simulator).is_empty()
    });
    let entries = entries(&harness);
    assert_eq!(entries.len(), 1, "{entries:?}");
    let change = &entries[0].changes[0];
    assert_eq!(entries[0].changes.len(), 1);
    assert_eq!(change.path.to_string(), LEVEL_PATH);
    assert_eq!(change.before.as_ref().and_then(Value::as_f64), Some(90.0));
    assert_eq!(change.after.as_f64(), Some(level_sets(&simulator)[0]));
}

#[test]
fn a_group_mute_is_one_entry_with_every_sent_member() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "group mute", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty() && !sets(&kh750, "/audio/out/mute").is_empty()
    });
    let entries = entries(&harness);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].changes.len(), 2);
}

#[test]
fn a_group_mute_leaves_an_unreachable_member_out_of_the_entry() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    kh750.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "the reachable member's mute", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty()
    });
    let entries = entries(&harness);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].changes.len(), 1);
}

#[test]
fn identify_is_not_recorded() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::IDENTIFY.get()).click();
    wait_until(&mut harness, "the identify write", |_| {
        !sets(&simulator, "/device/identification/visual").is_empty()
    });
    harness.run_steps(2);
    assert!(entries(&harness).is_empty());
}

#[test]
fn taking_back_writes_the_value_before() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::MUTE.get()).click();
    wait_until(&mut harness, "mute", |_| {
        sets(&simulator, "/audio/out/mute") == [json!(true)]
    });
    let id = entries(&harness)[0].id;
    take_back(&mut harness, id);
    wait_until(&mut harness, "unmute", |_| {
        sets(&simulator, "/audio/out/mute") == [json!(true), json!(false)]
    });
    let entries = entries(&harness);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].takes_back, Some(id));
    assert!(entries[1].changes[0].taken_back);
}

#[test]
fn a_take_back_is_itself_an_entry_that_can_be_taken_back() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::MUTE.get()).click();
    wait_until(&mut harness, "mute", |_| {
        !sets(&simulator, "/audio/out/mute").is_empty()
    });
    let first = entries(&harness)[0].id;
    take_back(&mut harness, first);
    wait_until(&mut harness, "unmute", |_| {
        sets(&simulator, "/audio/out/mute").len() == 2
    });
    let newest = harness.state().history().newest_open();
    assert_eq!(newest, Some(entries(&harness)[0].id));
    take_back(&mut harness, newest.unwrap());
    wait_until(&mut harness, "mute again", |_| {
        sets(&simulator, "/audio/out/mute") == [json!(true), json!(false), json!(true)]
    });
}

#[test]
fn a_changed_value_asks_before_taking_back() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    simulator.set_value(LEVEL_PATH, json!(90.0));
    wait_for_level(&mut harness, 90.0);
    drag_fader_down(&mut harness);
    wait_until(&mut harness, "the level write", |_| {
        !level_sets(&simulator).is_empty()
    });
    // Someone else turns the speaker meanwhile.
    simulator.set_value(LEVEL_PATH, json!(70.0));
    wait_for_level(&mut harness, 70.0);
    let id = entries(&harness)[0].id;
    take_back(&mut harness, id);
    assert_eq!(harness.state().take_back_question(), Some(id));
    harness.state_mut().answer_take_back(false);
    harness.run_steps(3);
    barrier(&simulator);
    assert_eq!(level_sets(&simulator).len(), 1);
    assert_eq!(harness.state().take_back_question(), None);
    take_back(&mut harness, id);
    harness.state_mut().answer_take_back(true);
    wait_until(&mut harness, "the level written back", |_| {
        level_sets(&simulator).len() == 2
    });
    assert_eq!(level_sets(&simulator)[1], 90.0);
}

#[test]
fn an_unreachable_speaker_stays_open() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "group mute", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty() && !sets(&kh750, "/audio/out/mute").is_empty()
    });
    kh750.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    let id = entries(&harness)[0].id;
    take_back(&mut harness, id);
    wait_until(&mut harness, "the reachable member unmuted", |_| {
        sets(&kh120, "/audio/out/mute").len() == 2
    });
    let original = harness.state().history().entry(id).unwrap().clone();
    let open: Vec<bool> = original.changes.iter().map(is_open).collect();
    assert_eq!(open.iter().filter(|open| **open).count(), 1, "{open:?}");
    assert!(harness.state().take_back_notice().is_some());
}
