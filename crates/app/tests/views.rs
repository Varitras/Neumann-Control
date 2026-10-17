//! The app window driven through egui_kittest, with real speaker workers talking to the
//! simulator. Threads deliver the events, so the tests step frames until a condition holds
//! (bounded by a deadline) instead of `run()`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, accesskit::Role, FontFamily, FontId, MouseWheelUnit, TouchPhase};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::spawner::{ScanResult, SpeakerSpawner};
use neumann_control_app::texts;
use neumann_control_app::ui_kit::fonts;
use neumann_control_core::address::AddressError;
use neumann_control_core::catalog::{Catalog, ValueError};
use neumann_control_core::discovery::Found;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::{self, ManualSpeaker, Settings, SETTINGS_FILE};
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::{self, Event, SpeakerHandle, Target, Timing};
use serde_json::Value;

const SIMULATED_SERIAL: &str = "SIM0001234";
const WAIT_LIMIT: Duration = Duration::from_secs(15);
// Paces the polling loop only; every outcome is decided by the condition.
const POLL_PAUSE: Duration = Duration::from_millis(10);
const WINDOW_SIZE: [f32; 2] = [900.0, 700.0];
const LOOPBACK: &str = "::1";

struct SimulatorSpawner {
    found: Vec<Found>,
}

impl SpeakerSpawner for SimulatorSpawner {
    fn spawn(
        &self,
        target: Target,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> (SpeakerHandle, Receiver<Event>) {
        speaker::spawn(target, Timing::default(), wake)
    }

    fn scan(&self) -> Receiver<ScanResult> {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(self.found.clone())).unwrap();
        receiver
    }
}

fn manual_for(simulator: &Simulator) -> ManualSpeaker {
    ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: simulator.address().port(),
        serial: None,
    }
}

fn settings_file(directory: &Path, manual: Vec<ManualSpeaker>) -> PathBuf {
    let path = directory.join(SETTINGS_FILE);
    let settings = Settings {
        manual,
        window: None,
    };
    settings::save(&settings, &path).unwrap();
    path
}

fn harness(settings_path: PathBuf, found: Vec<Found>) -> Harness<'static, NeumannApp> {
    Harness::builder()
        .with_size(WINDOW_SIZE)
        .build_eframe(move |creation_context| {
            NeumannApp::new(
                &creation_context.egui_ctx,
                settings_path,
                Box::new(SimulatorSpawner { found }),
            )
        })
}

fn wait_until(
    harness: &mut Harness<'static, NeumannApp>,
    what: &str,
    condition: impl Fn(&Harness<'static, NeumannApp>) -> bool,
) {
    let deadline = Instant::now() + WAIT_LIMIT;
    loop {
        harness.step();
        if condition(harness) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(POLL_PAUSE);
    }
}

fn readout(value: f64) -> String {
    texts::readout(value, "dB SPL")
}

fn wait_for_level(harness: &mut Harness<'static, NeumannApp>, value: f64) {
    let label = readout(value);
    wait_until(harness, &label, |harness| {
        harness.query_by_label(&label).is_some()
    });
}

/// Requests the simulator must receive after a frame before "nothing was sent" can be
/// asserted: the worker runs commands before polls, so one request still in flight plus one
/// more mean anything the frame sent has arrived.
const BARRIER_REQUESTS: usize = 2;

/// Waits without drawing frames, so the window sends nothing new meanwhile.
fn barrier(simulator: &Simulator) {
    let start = simulator.requests().len();
    let deadline = Instant::now() + WAIT_LIMIT;
    while simulator.requests().len() < start + BARRIER_REQUESTS {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for later polls"
        );
        thread::sleep(POLL_PAUSE);
    }
}

fn level_sets(simulator: &Simulator) -> Vec<f64> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| request.pointer("/audio/out/level").and_then(Value::as_f64))
        .collect()
}

fn connected_card(model: SimModel) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let simulator = Simulator::start(model);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut harness = harness(path, Vec::new());
    wait_for_level(&mut harness, 100.0);
    (simulator, directory, harness)
}

#[test]
fn card_shows_level_and_name() {
    let (_simulator, _directory, harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label("KH 120 II");
    harness.get_by_label("Simulated KH 120 II");
    harness.get_by_label(&readout(100.0));
    harness.get_by_label_contains(&mask_serial(SIMULATED_SERIAL));
    assert!(harness.query_by_label_contains(SIMULATED_SERIAL).is_none());
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn fader_drag_sends_once_on_release() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let rect = harness.get_by_role(Role::Slider).rect();
    let start = rect.center();
    harness.drag_at(start);
    for frame in 1..=10 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "a Set went out while dragging"
    );
    let shown = harness
        .get_by_role(Role::Slider)
        .accesskit_node()
        .numeric_value()
        .unwrap();
    harness.drop_at(start + egui::vec2(0.0, 40.0));
    wait_until(&mut harness, "the Set after release", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = level_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0] < 100.0, "dragging down lowers the level: {sets:?}");
    assert_eq!(shown, sets[0]);
}

#[test]
fn wheel_over_fader_sends_one_set_after_the_wheel_went_quiet() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let center = harness.get_by_role(Role::Slider).rect().center();
    harness.hover_at(center);
    for _ in 0..2 {
        harness.event(egui::Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
    barrier(&simulator);
    assert!(level_sets(&simulator).is_empty(), "the wheel wrote at once");
    harness.get_by_label(&readout(101.0));
    wait_until(&mut harness, "the wheel Set", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    assert_eq!(level_sets(&simulator), vec![101.0]);
}

#[test]
fn escape_during_a_drag_sends_nothing() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let start = harness.get_by_role(Role::Slider).rect().center();
    harness.drag_at(start);
    for frame in 1..=5 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    harness.key_press(egui::Key::Escape);
    harness.step();
    for frame in 6..=10 {
        harness.hover_at(start + egui::vec2(0.0, 4.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 40.0));
    harness.run_steps(10);
    barrier(&simulator);
    assert!(
        level_sets(&simulator).is_empty(),
        "{:?}",
        level_sets(&simulator)
    );
    harness.get_by_label(&readout(100.0));
    // The next drag is a normal one again.
    harness.drag_at(start);
    harness.hover_at(start + egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "the next drag's Set", |_| {
        !level_sets(&simulator).is_empty()
    });
}

#[test]
fn a_drag_replaces_wheel_steps_not_yet_sent() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    let start = harness.get_by_role(Role::Slider).rect().center();
    harness.hover_at(start);
    harness.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    harness.drag_at(start);
    for frame in 1..=4 {
        harness.hover_at(start + egui::vec2(0.0, 5.0 * frame as f32));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(0.0, 20.0));
    wait_until(&mut harness, "the drag's Set", |_| {
        !level_sets(&simulator).is_empty()
    });
    harness.run_steps(10);
    barrier(&simulator);
    let sets = level_sets(&simulator);
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert!(sets[0] < 100.0, "{sets:?}");
}

#[test]
fn mute_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::MUTE).click();
    wait_until(&mut harness, "mute on the device", |_| {
        simulator.value("audio/out/mute") == Some(Value::Bool(true))
    });
}

#[test]
fn identify_button_sends_set() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::IDENTIFY).click();
    wait_until(&mut harness, "identification on the device", |_| {
        simulator.value("device/identification/visual") == Some(Value::Bool(true))
    });
}

#[test]
fn more_toggles_to_less_and_back() {
    let (_simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(2);
    harness.get_by_label(texts::LESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::MORE);
}

#[test]
fn unreachable_card_is_disabled() {
    let (simulator, _directory, mut harness) = connected_card(SimModel::Kh120Ii);
    simulator.stop();
    wait_until(&mut harness, "the unreachable text", |harness| {
        harness.query_by_label(texts::UNREACHABLE).is_some()
    });
    assert!(harness
        .get_by_label(texts::MUTE)
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_label(texts::IDENTIFY)
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_role(Role::Slider)
        .accesskit_node()
        .is_disabled());
}

#[test]
fn add_address_refuses_ipv4_with_hint() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text("192.168.1.20");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::describe_address_error(
        &AddressError::Ipv4NotSupported,
    ));
    assert!(settings::load(&path).manual.is_empty());
    harness.get_by_label(&texts::speaker_count(0));
}

#[test]
fn add_address_saves_the_speaker_and_shows_its_card() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text(LOOPBACK);
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::ADD_ADDRESS_TITLE).is_none());
    let saved = settings::load(&path).manual;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].address, LOOPBACK);
    assert_eq!(saved[0].port, neumann_control_core::address::SSC_PORT);
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn scan_adds_found_speakers_once() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let found = Found {
        name: String::new(),
        product: Some("KH 750".to_owned()),
        serial: Some("SIM0007500".to_owned()),
        address: simulator.address(),
    };
    let mut harness = harness(path, vec![found]);
    harness.get_by_label(texts::SCAN).click();
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label("KH 750");
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(2);
    harness.get_by_label(texts::SCAN).click();
    harness.run_steps(5);
    harness.get_by_label(&texts::speaker_count(1));
    // The card kept its state, so it was not recreated.
    harness.get_by_label(texts::LESS);
}

/// Every string literal in texts.rs, so a new text is covered without being listed here.
fn literals_of_texts_module() -> Vec<String> {
    let source = include_str!("../src/texts.rs");
    let mut literals = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let end = after.find('"').expect("closed literal");
        literals.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    literals
}

#[test]
fn all_ui_texts_have_glyphs() {
    let context = egui::Context::default();
    fonts::install(&context);
    context
        .run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
    let catalog = Catalog::embedded();
    let mut texts_to_check = literals_of_texts_module();
    texts_to_check.extend([
        texts::speaker_count(12),
        texts::port(45),
        texts::readout(-12.5, "dB SPL"),
        texts::status_line("1_7_3", &mask_serial(SIMULATED_SERIAL)),
        texts::serial_mismatch(&mask_serial(SIMULATED_SERIAL)),
        texts::describe_rejection(405),
        texts::describe_rejection(406),
        texts::describe_rejection(500),
    ]);
    for error in [
        AddressError::Ipv4NotSupported,
        AddressError::MissingScope,
        AddressError::ScopeNotNumeric,
        AddressError::Invalid,
    ] {
        texts_to_check.push(texts::describe_address_error(&error));
    }
    for error in [
        ValueError::OutOfRange { min: 0.0, max: 1.0 },
        ValueError::NotAnOption,
        ValueError::NotAStep,
        ValueError::TooLong(52),
        ValueError::NotPrintableAscii,
        ValueError::WrongType,
    ] {
        texts_to_check.push(texts::describe_value_error(&error));
    }
    for entry in &catalog.entries {
        texts_to_check.push(entry.group.title().to_owned());
        texts_to_check.push(entry.label.clone());
        texts_to_check.extend(entry.unit.clone());
    }
    let families = [FontFamily::Proportional, fonts::semibold(12.0).family];
    for family in families {
        let font = FontId::new(12.0, family.clone());
        for text in &texts_to_check {
            let covered = context.fonts_mut(|fonts| fonts.has_glyphs(&font, text));
            assert!(covered, "{text:?} in {family:?}");
        }
    }
    assert!(texts_to_check.iter().any(|text| text == texts::NO_SPEAKERS));
}

#[test]
fn scan_moves_a_known_speaker_to_its_new_address() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let stale = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: neumann_control_core::address::SSC_PORT,
        serial: Some(SIMULATED_SERIAL.to_owned()),
    };
    let path = settings_file(directory.path(), vec![stale]);
    let found = Found {
        name: "announced".to_owned(),
        product: None,
        serial: Some(SIMULATED_SERIAL.to_owned()),
        address: simulator.address(),
    };
    let mut harness = harness(path, vec![found]);
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label(texts::SCAN).click();
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label(&texts::speaker_count(1));
}

fn readouts(harness: &Harness<'static, NeumannApp>) -> usize {
    harness.query_all_by_label(&readout(100.0)).count()
}

fn has_one_card(harness: &Harness<'static, NeumannApp>) -> bool {
    readouts(harness) == 1 && harness.query_by_label(&texts::speaker_count(1)).is_some()
}

fn serials_remembered(path: &Path, count: usize) -> bool {
    let manual = settings::load(path).manual;
    manual.len() == count
        && manual
            .iter()
            .all(|entry| entry.serial.as_deref() == Some(SIMULATED_SERIAL))
}

#[test]
fn a_manual_speaker_found_at_a_second_address_stays_one_card() {
    let manual_simulator = Simulator::start(SimModel::Kh120Ii);
    // The manual card connects only after the scan created a card for the second address.
    manual_simulator.set_reply_delay(Duration::from_millis(300));
    let second_simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&manual_simulator)]);
    let found = Found {
        name: String::new(),
        product: Some("KH 120 II".to_owned()),
        serial: Some(SIMULATED_SERIAL.to_owned()),
        address: second_simulator.address(),
    };
    let mut harness = harness(path.clone(), vec![found]);
    harness.get_by_label(texts::SCAN).click();
    wait_until(&mut harness, "the serial in the manual entry", |_| {
        serials_remembered(&path, 1)
    });
    wait_until(&mut harness, "one card after the first scan", has_one_card);
    harness.get_by_label(texts::SCAN).click();
    harness.run_steps(5);
    wait_until(&mut harness, "one card after the second scan", has_one_card);
}

#[test]
fn two_manual_entries_for_one_speaker_stay_one_card() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let second = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(
        directory.path(),
        vec![manual_for(&first), manual_for(&second)],
    );
    let mut harness = harness(path.clone(), Vec::new());
    wait_until(&mut harness, "both serials remembered", |_| {
        serials_remembered(&path, 2)
    });
    wait_until(&mut harness, "one card", has_one_card);
}

#[test]
fn add_address_names_an_address_already_listed() {
    let directory = tempfile::tempdir().unwrap();
    let listed = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: neumann_control_core::address::SSC_PORT,
        serial: None,
    };
    let path = settings_file(directory.path(), vec![listed]);
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text("[::1]");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_ALREADY_LISTED);
    harness.get_by_label(texts::ADD_ADDRESS_TITLE);
    assert_eq!(settings::load(&path).manual.len(), 1);
}
