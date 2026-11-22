//! The Windows volume strip, with a fake standing in for Windows' playback devices: shown only
//! when chosen, it reads name, volume, mute and peaks and hands mute and fader moves to Windows.

use crate::support::*;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::system_audio::{AudioDevice, AudioState};
use neumann_control_app::texts;
use neumann_control_core::settings::{self, SystemAudioSettings};
use neumann_control_core::simulator::{SimulatedModel, Simulator};

const DEVICE_NAME: &str = "Studio monitors";
const DEVICE_ID: &str = "{0.0.0.00000000}.{fake}";

fn fake(state: Option<AudioState>) -> FakeAudio {
    let audio = FakeAudio::default();
    {
        let mut held = audio.0.lock().unwrap();
        held.devices = vec![AudioDevice {
            id: DEVICE_ID.to_owned(),
            name: DEVICE_NAME.to_owned(),
        }];
        held.state = state;
    }
    audio
}

fn playing() -> AudioState {
    AudioState {
        name: DEVICE_NAME.to_owned(),
        volume: 0.5,
        mute: false,
        peaks: vec![0.5, 0.25],
        channel_mask: None,
    }
}

/// A connected KH 120 II, the strip shown or not, and the fake behind it.
fn desk_with(
    shown: bool,
    audio: FakeAudio,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    desk_on(shown, None, audio)
}

/// Like `desk_with`, with `device` chosen in the settings.
fn desk_on(
    shown: bool,
    device: Option<&str>,
    audio: FakeAudio,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let simulator = Simulator::start(SimulatedModel::Kh120Ii);
    simulator.set_reply_delay(REPLY_DELAY);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut stored = settings::load(&path);
    stored.system_audio = SystemAudioSettings {
        shown,
        device: device.map(str::to_owned),
    };
    settings::save(&stored, &path).unwrap();
    let mut harness = harness_with_audio(path, audio);
    harness.set_size(TWO_STRIP_WINDOW.into());
    wait_for_level(&mut harness, 100.0);
    (simulator, directory, harness)
}

/// No speaker yet, the strip chosen or not, the master shown if there were any.
fn empty_desk(shown: bool, audio: FakeAudio) -> (tempfile::TempDir, Harness<'static, NeumannApp>) {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut stored = settings::load(&path);
    stored.system_audio = SystemAudioSettings {
        shown,
        device: None,
    };
    stored.desk.show_master = true;
    settings::save(&stored, &path).unwrap();
    let mut harness = harness_with_audio(path, audio);
    harness.run_steps(3);
    (directory, harness)
}

/// The strip needs no speaker: it was missing until one was added, with the desk saying only
/// that there are none.
#[test]
fn the_strip_shows_on_a_desk_without_speakers() {
    let (_directory, harness) = empty_desk(true, fake(Some(playing())));
    harness.get_by_label(texts::NO_SPEAKERS.get());
    harness.get_by_label(DEVICE_NAME);
    assert!(harness.query_by_label(texts::MASTER.get()).is_none());
}

/// Without speakers a group has no member on the desk: its strip stays away like the master's.
/// It showed beside the Windows strip with nothing to control.
#[test]
fn an_empty_desk_shows_no_group_strip() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut stored = settings::load(&path);
    stored.system_audio = SystemAudioSettings {
        shown: true,
        device: None,
    };
    let group = serde_json::json!({"name": "Front", "members": ["serial:SIM9999999"]});
    stored.groups.push(serde_json::from_value(group).unwrap());
    settings::save(&stored, &path).unwrap();
    let mut harness = harness_with_audio(path, fake(Some(playing())));
    harness.run_steps(3);
    harness.get_by_label(DEVICE_NAME);
    assert!(harness.query_by_label("Front").is_none());
}

#[test]
fn a_desk_without_speakers_leaves_a_strip_switched_off_away() {
    let audio = fake(Some(playing()));
    let (_directory, harness) = empty_desk(false, audio.clone());
    harness.get_by_label(texts::NO_SPEAKERS.get());
    assert!(harness.query_by_label(DEVICE_NAME).is_none());
    assert!(audio.0.lock().unwrap().asked.is_empty());
}

#[test]
fn the_strip_is_off_until_chosen() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, harness) = desk_with(false, audio.clone());
    assert!(harness.query_by_label(DEVICE_NAME).is_none());
    assert!(audio.0.lock().unwrap().asked.is_empty());
}

/// Linux has no volume strip: switched on in a settings file from Windows it stays away, and
/// the settings do not offer it.
#[test]
fn a_system_without_the_strip_neither_shows_nor_offers_it() {
    let audio = fake(Some(playing()));
    audio.0.lock().unwrap().missing = true;
    let (_simulator, _directory, mut harness) = desk_with(true, audio.clone());
    harness.run_steps(2);
    assert!(harness.query_by_label(DEVICE_NAME).is_none());
    assert!(audio.0.lock().unwrap().asked.is_empty());
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::DETAILED_LOG.get());
    assert!(harness
        .query_by_label(texts::SYSTEM_AUDIO_SHOW.get())
        .is_none());
}

#[test]
fn the_strip_shows_the_default_device() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_with(true, audio.clone());
    harness.run_steps(2);
    harness.get_by_label(DEVICE_NAME);
    harness.get_by_label(&texts::percent(50.0));
    assert_eq!(audio.0.lock().unwrap().asked.last(), Some(&None));
}

#[test]
fn mute_goes_to_windows() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_with(true, audio.clone());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::SYSTEM_MUTE.get())
        .click();
    harness.run_steps(2);
    assert_eq!(audio.0.lock().unwrap().mute_sets, vec![true]);
}

#[test]
fn a_fader_move_sets_the_windows_volume() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_with(true, audio.clone());
    harness.run_steps(2);
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::SYSTEM_VOLUME.get())
        .rect();
    let from = fader.center();
    harness.drag_at(from);
    for frame in 1..=5 {
        harness.hover_at(from + egui::vec2(0.0, -8.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(from + egui::vec2(0.0, -40.0));
    harness.run_steps(3);
    let sets = audio.0.lock().unwrap().volume_sets.clone();
    assert_eq!(sets.len(), 1, "{sets:?}");
    // Windows takes a scalar: up from 50 %, never past 1.
    assert!(sets[0] > 0.5 && sets[0] <= 1.0, "{sets:?}");
}

/// The fader is in percent, and so is the move it shows while dragged: it said "+12,0 dB".
#[test]
fn a_dragged_windows_fader_shows_its_move_in_percent() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_with(true, audio);
    harness.run_steps(2);
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::SYSTEM_VOLUME.get())
        .rect();
    let from = fader.center();
    harness.drag_at(from);
    for frame in 1..=5 {
        harness.hover_at(from + egui::vec2(0.0, -8.0 * frame as f32));
        harness.step();
    }
    let moves: Vec<String> = harness
        .query_all_by_role(Role::Label)
        .filter_map(|node| node.value())
        .filter(|text| text.starts_with('+'))
        .collect();
    harness.drop_at(from + egui::vec2(0.0, -40.0));
    assert!(
        moves.iter().any(|text| text.ends_with(" %"))
            && !moves.iter().any(|text| text.ends_with("dB")),
        "{moves:?}"
    );
}

/// Dragged past the top, the volume ends at 100 %: Windows' scalar 1.
#[test]
fn a_fader_past_the_top_sets_full_volume() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_with(true, audio.clone());
    harness.run_steps(2);
    let fader = harness
        .get_by_role_and_label(Role::Slider, texts::SYSTEM_VOLUME.get())
        .rect();
    let from = fader.center();
    harness.drag_at(from);
    for frame in 1..=5 {
        harness.hover_at(from + egui::vec2(0.0, -200.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(from + egui::vec2(0.0, -1000.0));
    harness.run_steps(3);
    assert_eq!(audio.0.lock().unwrap().volume_sets, vec![1.0]);
}

/// With a device chosen, its volume and mute change, not Windows' default device.
#[test]
fn mute_goes_to_the_chosen_device() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, mut harness) = desk_on(true, Some(DEVICE_ID), audio.clone());
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, texts::SYSTEM_MUTE.get())
        .click();
    harness.run_steps(2);
    assert_eq!(
        audio.0.lock().unwrap().set_on,
        vec![Some(DEVICE_ID.to_owned())]
    );
}

#[test]
fn a_vanished_device_reads_unavailable() {
    let audio = fake(None);
    let (_simulator, _directory, mut harness) = desk_with(true, audio);
    harness.run_steps(2);
    harness.get_by_label(texts::SYSTEM_AUDIO_UNAVAILABLE.get());
    assert!(harness
        .query_by_role_and_label(Role::Slider, texts::SYSTEM_VOLUME.get())
        .is_none());
}

#[test]
fn the_device_is_chosen_in_the_settings() {
    let audio = fake(Some(playing()));
    let (_simulator, directory, mut harness) = desk_with(true, audio);
    harness.get_by_label(texts::SETTINGS.get()).click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::ComboBox, texts::SYSTEM_AUDIO_DEVICE.get())
        .click();
    harness.run_steps(2);
    harness
        .query_all_by_label(DEVICE_NAME)
        .find(|node| node.accesskit_node().role() != Role::Label)
        .expect("the device in the list")
        .click();
    harness.run_steps(2);
    let stored = settings::load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(stored.system_audio.device.as_deref(), Some(DEVICE_ID));
}
