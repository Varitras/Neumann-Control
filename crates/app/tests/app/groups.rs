//! Groups on the desk: group fader, group and master mute, linked settings, the group dialog
//! and choosing a group in the device panel.

use crate::support::*;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::groups::linkable_label;
use neumann_control_app::texts;
use neumann_control_app::writes::Refusal;
use neumann_control_core::settings::{self, GroupSettings, Linkable, Settings, SETTINGS_FILE};
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::Value;

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
        .get_all_by_label(texts::MUTE.get())
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
        .get_all_by_label(texts::MUTE.get())
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
    harness.get_by_label(texts::MUTE_ALL.get()).click();
    let unmuted = [Value::Bool(true), Value::Bool(false)];
    wait_until(&mut harness, "master unmute", |_| {
        sets(&kh120, "/audio/out/mute") == unmuted && sets(&kh750, "/audio/out/mute") == unmuted
    });
}

#[test]
fn link_buttons_toggle_and_are_saved() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    harness
        .get_by_label(&texts::link_label(linkable_label(Linkable::Level)))
        .click();
    harness.run_steps(2);
    let path = directory.path().join(SETTINGS_FILE);
    assert_eq!(
        settings::load(&path).groups[0].linked,
        vec![Linkable::Level]
    );
    harness
        .get_by_label(&texts::link_label(linkable_label(Linkable::Level)))
        .click();
    harness.run_steps(2);
    assert!(settings::load(&path).groups[0].linked.is_empty());
}

#[test]
fn group_dialog_renames_and_deletes() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    harness.get_by_label(GROUP_NAME).click();
    wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE.get());
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("Hinten");
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    let path = directory.path().join(SETTINGS_FILE);
    assert_eq!(settings::load(&path).groups[0].name, "Hinten");
    harness.get_by_label("Hinten").click();
    wait_for_label(&mut harness, texts::DELETE_GROUP.get());
    harness.get_by_label(texts::DELETE_GROUP.get()).click();
    harness.run_steps(2);
    assert!(settings::load(&path).groups.is_empty());
}

#[test]
fn add_group_creates_one_and_the_panel_assigns_a_speaker() {
    let (simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.get_by_label(texts::ADD_GROUP.get()).click();
    wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE.get());
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    let path = directory.path().join(SETTINGS_FILE);
    assert_eq!(settings::load(&path).groups.len(), 1);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness
        .get_by_role_and_label(Role::ComboBox, texts::GROUP.get())
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
        .query_all_by_label(texts::MOVE_STRIP.get())
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

#[test]
fn a_strip_can_be_dragged_behind_the_last_one_and_the_others_make_room_meanwhile() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    let handle = harness
        .get_all_by_label(texts::MOVE_STRIP.get())
        .next()
        .unwrap()
        .rect();
    let group_strip = harness
        .get_by_role_and_label(Role::Slider, &texts::group_level(GROUP_NAME))
        .rect();
    // Past the last member, over the group strip: no strip to drop "in front of" there.
    let target = egui::pos2(group_strip.center().x, handle.center().y);
    harness.drag_at(handle.center());
    for frame in 1..=6 {
        let progress = frame as f32 / 6.0;
        harness.hover_at(handle.center() + (target - handle.center()) * progress);
        harness.step();
    }
    let x_of = |harness: &Harness<'static, NeumannApp>, name: &str| {
        harness.get_by_label(name).rect().center().x
    };
    assert!(
        x_of(&harness, KH750_DEVICE_NAME) < x_of(&harness, KH120_DEVICE_NAME),
        "the other strip moves aside while dragging"
    );
    harness.drop_at(target);
    harness.run_steps(2);
    let saved = settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(saved.order, vec![KH750_ID.to_owned(), KH120_ID.to_owned()]);
}

#[test]
fn escape_while_dragging_keeps_the_order() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    let handles: Vec<egui::Rect> = harness
        .query_all_by_label(texts::MOVE_STRIP.get())
        .map(|node| node.rect())
        .collect();
    let (first, second) = (handles[0], handles[1]);
    harness.drag_at(second.center());
    for frame in 1..=6 {
        let progress = frame as f32 / 6.0;
        harness.hover_at(second.center() + (first.center() - second.center()) * progress);
        harness.step();
    }
    harness.key_press(egui::Key::Escape);
    harness.drop_at(first.center());
    harness.run_steps(2);
    let saved = settings::load(&directory.path().join(SETTINGS_FILE));
    assert!(saved.order.is_empty(), "{:?}", saved.order);
}

#[test]
fn order_and_group_saved_by_address_follow_the_speaker_once_it_connects() {
    let kh120 = Simulator::start(SimModel::Kh120Ii);
    let kh750 = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    // Added by address and never connected yet: no serial is known.
    let address_id =
        |simulator: &Simulator| format!("address:[::1]:{}", simulator.address().port());
    let settings = Settings {
        manual: vec![manual_for(&kh120), manual_for(&kh750)],
        groups: vec![GroupSettings {
            name: GROUP_NAME.to_owned(),
            color: 0,
            members: vec![address_id(&kh120)],
            linked: Vec::new(),
        }],
        order: vec![address_id(&kh750), address_id(&kh120)],
        ..Settings::default()
    };
    settings::save(&settings, &path).unwrap();
    let mut harness = harness(path.clone(), Vec::new());
    harness.set_size(egui::vec2(1200.0, 900.0));
    wait_until(&mut harness, "both strips connected", |harness| {
        harness.query_all_by_label(&readout(100.0)).count() == 2
    });
    harness.run_steps(2);
    let saved = settings::load(&path);
    assert_eq!(saved.order, vec![KH750_ID.to_owned(), KH120_ID.to_owned()]);
    assert_eq!(saved.groups[0].members, vec![KH120_ID.to_owned()]);
}

#[test]
fn removing_a_speaker_also_forgets_its_group() {
    let (_kh120, _kh750, directory, mut harness) = grouped(Vec::new());
    open_panel(&mut harness, KH120_DEVICE_NAME);
    click_in_panel(&mut harness, texts::REMOVE.get());
    wait_for_label(&mut harness, texts::REMOVE_TITLE.get());
    harness
        .query_all_by_label(texts::REMOVE.get())
        .last()
        .expect("the dialog's button")
        .click();
    harness.run_steps(2);
    let saved = settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(saved.groups[0].members, vec![KH750_ID.to_owned()]);
}

#[test]
fn group_mute_leaves_an_unreachable_member_alone() {
    let (kh120, kh750, _directory, mut harness) = grouped(Vec::new());
    kh750.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    harness.get_by_label(&texts::group_mute(GROUP_NAME)).click();
    wait_until(&mut harness, "the reachable member's mute", |_| {
        !sets(&kh120, "/audio/out/mute").is_empty()
    });
    harness.run_steps(2);
    harness.get_by_label(texts::UNREACHABLE.get());
}

#[test]
fn a_new_group_never_repeats_the_name_or_colour_of_another() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let path = directory.path().join(SETTINGS_FILE);
    for _ in 0..2 {
        harness.get_by_label(texts::ADD_GROUP.get()).click();
        wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE.get());
        harness.get_by_label(texts::CLOSE.get()).click();
        harness.run_steps(2);
    }
    let first = settings::load(&path).groups[0].name.clone();
    harness.get_by_label(&first).click();
    wait_for_label(&mut harness, texts::DELETE_GROUP.get());
    harness.get_by_label(texts::DELETE_GROUP.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADD_GROUP.get()).click();
    wait_for_label(&mut harness, texts::GROUP_DIALOG_TITLE.get());
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    let groups = settings::load(&path).groups;
    assert_eq!(groups.len(), 2);
    assert_ne!(groups[0].name, groups[1].name);
    assert_ne!(groups[0].color, groups[1].color);
}

#[test]
fn a_linked_setting_skips_a_model_that_may_not_write_it() {
    // The KH 750's auto standby follows its rear switch and refuses writes.
    let (kh120, kh750, _directory, mut harness) = grouped(vec![Linkable::Standby]);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness
        .get_by_role_and_label(Role::CheckBox, "Auto-Standby")
        .click();
    wait_until(&mut harness, "the KH 120 II standby set", |_| {
        !sets(&kh120, "/device/standby/enabled").is_empty()
    });
    harness.run_steps(5);
    barrier(&kh750);
    assert!(sets(&kh750, "/device/standby/enabled").is_empty());
    harness.get_by_label(&texts::describe_refusal(&Refusal::NotWritable));
}

#[test]
fn a_linked_level_reaches_the_other_member_as_it_is() {
    let (kh120, kh750, _directory, mut harness) = grouped(vec![Linkable::Level]);
    let start = harness
        .query_all_by_role_and_label(Role::Slider, texts::LEVEL.get())
        .next()
        .unwrap()
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
    assert_eq!(
        sets(&kh120, "/audio/out/level"),
        sets(&kh750, "/audio/out/level")
    );
}

#[test]
fn copy_to_group_sends_the_members_value_to_the_others() {
    let (kh120, kh750, _directory, mut harness) = grouped(vec![Linkable::Level]);
    kh120.set_value("audio/out/level", serde_json::json!(90.0));
    wait_for_level(&mut harness, 90.0);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    let copy = texts::copy_to_group(linkable_label(Linkable::Level));
    harness.get_by_label(&copy).click();
    wait_until(&mut harness, "the copied level", |_| {
        !sets(&kh750, "/audio/out/level").is_empty()
    });
    assert_eq!(
        sets(&kh750, "/audio/out/level"),
        vec![serde_json::json!(90.0)]
    );
    assert!(sets(&kh120, "/audio/out/level").is_empty());
}

#[test]
fn ungrouped_strips_can_be_reordered_too() {
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
        ..Settings::default()
    };
    settings::save(&settings, &path).unwrap();
    let mut harness = harness(path.clone(), Vec::new());
    harness.set_size(egui::vec2(1200.0, 900.0));
    wait_until(&mut harness, "both strips connected", |harness| {
        harness.query_all_by_label(&readout(100.0)).count() == 2
    });
    let handles: Vec<egui::Rect> = harness
        .query_all_by_label(texts::MOVE_STRIP.get())
        .map(|node| node.rect())
        .collect();
    let (left, right) = (handles[0], handles[1]);
    let target = egui::pos2(right.right() + 200.0, left.center().y);
    harness.drag_at(left.center());
    for frame in 1..=6 {
        let progress = frame as f32 / 6.0;
        harness.hover_at(left.center() + (target - left.center()) * progress);
        harness.step();
    }
    harness.drop_at(target);
    harness.run_steps(2);
    assert_eq!(
        settings::load(&path).order,
        vec![KH750_ID.to_owned(), KH120_ID.to_owned()]
    );
}
