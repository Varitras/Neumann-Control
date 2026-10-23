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
use neumann_control_core::simulator::{SimModel, Simulator};

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
    }
}

/// A connected KH 120 II, the strip shown or not, and the fake behind it.
fn desk_with(
    shown: bool,
    audio: FakeAudio,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_reply_delay(REPLY_DELAY);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut stored = settings::load(&path);
    stored.system_audio = SystemAudioSettings {
        shown,
        device: None,
    };
    settings::save(&stored, &path).unwrap();
    let mut harness = harness_with_audio(path, audio);
    harness.set_size(egui::vec2(1200.0, 900.0));
    wait_for_level(&mut harness, 100.0);
    (simulator, directory, harness)
}

#[test]
fn the_strip_is_off_until_chosen() {
    let audio = fake(Some(playing()));
    let (_simulator, _directory, harness) = desk_with(false, audio.clone());
    assert!(harness.query_by_label(DEVICE_NAME).is_none());
    assert!(audio.0.lock().unwrap().asked.is_empty());
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
    assert!(sets[0] > 0.5, "{sets:?}");
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
    let stored = settings::load(&directory.path().join("settings.json"));
    assert_eq!(stored.system_audio.device.as_deref(), Some(DEVICE_ID));
}
