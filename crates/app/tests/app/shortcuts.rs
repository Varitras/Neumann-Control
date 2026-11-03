//! Keyboard shortcuts of the desk: master up and down, mute all, dim.

use crate::support::*;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::shortcuts::DeskShortcut;
use neumann_control_app::texts;
use neumann_control_core::simulator::{SimModel, Simulator};
use serde_json::Value;

fn press(harness: &mut Harness<'static, NeumannApp>, key: Key) {
    harness.key_press_modifiers(Modifiers::COMMAND, key);
    harness.run_steps(2);
}

fn mute_sets(simulator: &Simulator) -> Vec<Value> {
    sets(simulator, MUTE_POINTER)
}

#[test]
fn ctrl_up_and_down_move_every_speaker_by_one_db() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press(&mut harness, Key::ArrowUp);
    wait_until(&mut harness, "the level raised", |_| {
        level_sets(&simulator) == [101.0]
    });
    writes_answered(&mut harness, &simulator);
    press(&mut harness, Key::ArrowDown);
    wait_until(&mut harness, "the level lowered", |_| {
        level_sets(&simulator) == [101.0, 100.0]
    });
}

#[test]
fn ctrl_m_mutes_all_and_again_unmutes() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press(&mut harness, Key::M);
    wait_until(&mut harness, "all muted", |_| {
        mute_sets(&simulator) == [Value::Bool(true)]
    });
    writes_answered(&mut harness, &simulator);
    press(&mut harness, Key::M);
    wait_until(&mut harness, "all unmuted", |_| {
        mute_sets(&simulator) == [Value::Bool(true), Value::Bool(false)]
    });
}

/// Dim lowers by 20 dB; dim again goes back to exactly the levels before, also where the range
/// stopped the way down.
#[test]
fn ctrl_d_dims_and_restores_the_levels_before() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    press(&mut harness, Key::D);
    wait_until(&mut harness, "dimmed", |_| level_sets(&simulator) == [80.0]);
    writes_answered(&mut harness, &simulator);
    harness.get_by_label(texts::DIM.get());
    press(&mut harness, Key::D);
    wait_until(&mut harness, "restored", |_| {
        level_sets(&simulator) == [80.0, 100.0]
    });
}

/// A speaker added by address gets its serial key while dimmed (at a reconnect): dim off still
/// brings its level back, the dim state follows the key.
#[test]
fn dim_goes_back_after_the_speaker_got_its_serial() {
    let (simulator, directory, mut harness) =
        connected_speaker_with(SimModel::Kh120Ii, |simulator| {
            simulator.set_value("device/identity/serial", serde_json::json!(7));
        });
    press(&mut harness, Key::D);
    wait_until(&mut harness, "dimmed", |_| level_sets(&simulator) == [80.0]);
    writes_answered(&mut harness, &simulator);
    simulator.set_value("device/identity/serial", serde_json::json!("SIM0001234"));
    simulator.inject(neumann_control_core::simulator::Fault::CloseConnection);
    let settings = directory
        .path()
        .join(neumann_control_core::settings::SETTINGS_FILE);
    wait_until(&mut harness, "the serial kept", |_| {
        let stored = neumann_control_core::settings::load(&settings);
        stored
            .manual
            .iter()
            .any(|entry| entry.serial.as_deref() == Some("SIM0001234"))
    });
    press(&mut harness, Key::D);
    wait_until(&mut harness, "restored", |_| {
        level_sets(&simulator) == [80.0, 100.0]
    });
}

/// Typing in a field keeps its keys: no shortcut fires while a text field has the focus.
#[test]
fn a_shortcut_waits_while_a_field_is_edited() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .click();
    harness.run_steps(1);
    press(&mut harness, Key::M);
    harness.run_steps(5);
    barrier(&simulator);
    assert!(mute_sets(&simulator).is_empty());
}

/// Switched on in the settings, a shortcut pressed in another program reaches the desk;
/// switched off, the app stops listening.
#[test]
fn a_global_shortcut_reaches_the_desk_once_switched_on() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let keys = FakeGlobalKeys::default();
    harness.state_mut().set_global_keys(Box::new(keys.clone()));
    harness.run_steps(2);
    assert!(!keys.is_listening());
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(2);
    scroll_to_and_click(&mut harness, texts::GLOBAL_SHORTCUTS.get());
    harness.run_steps(2);
    assert!(keys.is_listening());
    keys.press(DeskShortcut::MuteAll);
    wait_until(&mut harness, "all muted", |_| {
        mute_sets(&simulator) == [Value::Bool(true)]
    });
    scroll_to_and_click(&mut harness, texts::GLOBAL_SHORTCUTS.get());
    harness.run_steps(2);
    assert!(!keys.is_listening());
}

/// Linux has no shortcuts from other programs: the settings do not offer them.
#[test]
fn the_settings_offer_no_global_shortcuts_where_the_system_has_none() {
    let (_simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let mut keys = FakeGlobalKeys::default();
    keys.missing = true;
    harness.state_mut().set_global_keys(Box::new(keys));
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::DETAILED_LOG.get());
    assert!(harness
        .query_by_label(texts::GLOBAL_SHORTCUTS.get())
        .is_none());
}

/// A speaker cut off when dim goes off gets its level back once it is reached again, instead of
/// staying 20 dB down.
#[test]
fn a_speaker_away_when_dim_goes_off_gets_its_level_back_on_return() {
    let (kh120, kh750, _directory, mut harness) = two_speakers(Vec::new());
    press(&mut harness, Key::D);
    wait_until(&mut harness, "both dimmed", |_| {
        level_sets(&kh120) == [80.0] && level_sets(&kh750) == [80.0]
    });
    writes_answered(&mut harness, &kh120);
    writes_answered(&mut harness, &kh750);
    let address = kh750.address();
    kh750.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    press(&mut harness, Key::D);
    wait_until(&mut harness, "the reachable one back", |_| {
        level_sets(&kh120) == [80.0, 100.0]
    });
    let back = Simulator::start_on(SimModel::Kh750, address).unwrap();
    wait_until(&mut harness, "the returned one back", |_| {
        level_sets(&back) == [100.0]
    });
}

/// A button reached with Tab takes no keys: the shortcuts still work (keyboard and screen
/// reader users move by Tab).
#[test]
fn a_shortcut_works_while_a_button_has_the_focus() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    harness.key_press(Key::Tab);
    harness.run_steps(2);
    let focused = harness.ctx.memory(|memory| memory.focused().is_some());
    assert!(focused, "Tab gave a button the focus");
    press(&mut harness, Key::M);
    wait_until(&mut harness, "all muted", |_| {
        mute_sets(&simulator) == [Value::Bool(true)]
    });
}

/// While the window is hidden only the app's logic runs, not its drawing: a shortcut written
/// then, whose write fails before the window is drawn again, is filed as not written, not as a
/// change to take back.
#[test]
fn a_shortcut_write_failed_while_hidden_is_filed_as_not_written() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let keys = FakeGlobalKeys::default();
    harness.state_mut().set_global_keys(Box::new(keys.clone()));
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(2);
    scroll_to_and_click(&mut harness, texts::GLOBAL_SHORTCUTS.get());
    harness.run_steps(2);
    simulator.inject(neumann_control_core::simulator::Fault::CloseConnection);
    keys.press(DeskShortcut::MuteAll);
    let context = harness.ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    let mut hidden_frame = |harness: &mut Harness<'static, NeumannApp>| {
        eframe::App::logic(harness.state_mut(), &context, &mut frame);
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while mute_sets(&simulator).is_empty() {
        assert!(std::time::Instant::now() < deadline, "no mute sent");
        hidden_frame(&mut harness);
    }
    // The reconnect's first read comes after the failed write's answer.
    let sent = simulator.requests().len();
    while simulator.requests().len() == sent {
        assert!(std::time::Instant::now() < deadline, "no reconnect");
        hidden_frame(&mut harness);
    }
    hidden_frame(&mut harness);
    harness.run_steps(1);
    let entry = harness.state().history().newest(0).expect("the mute filed");
    assert!(
        entry.changes.iter().all(|change| change.refused),
        "{entry:?}"
    );
}
