//! Shared by the window tests: the app with real speaker workers talking to the simulator.
//! Threads deliver the events, so tests step frames until a condition holds (bounded by a
//! deadline) instead of `run()`.
// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::spawner::{ScanResult, SpeakerSpawner};
use neumann_control_app::texts;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::{
    self, GroupSettings, Linkable, ManualSpeaker, Settings, SETTINGS_FILE,
};
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::{self, Events, SpeakerHandle, Target, Timing};
use serde_json::Value;

pub const SIMULATED_SERIAL: &str = "SIM0001234";
pub const KH120_DEVICE_NAME: &str = "Simulated KH 120 II";
pub const KH750_DEVICE_NAME: &str = "Simulated KH 750";
pub const WAIT_LIMIT: Duration = Duration::from_secs(15);
// Paces the polling loop only; every outcome is decided by the condition.
pub const POLL_PAUSE: Duration = Duration::from_millis(10);
pub const WINDOW_SIZE: [f32; 2] = [900.0, 700.0];
/// Tall enough for an open device panel, so every control in it can be clicked.
pub const EXPANDED_WINDOW_SIZE: [f32; 2] = [1200.0, 1600.0];
pub const LOOPBACK: &str = "::1";
const DIALOG_SETTLE_FRAMES: usize = 2;
const PRODUCT_PATH: &str = "device/identity/product";
/// Real speakers take tens of milliseconds per reply; an instant simulator hid tests that
/// acted before the connect-time read had arrived.
const REPLY_DELAY: Duration = Duration::from_millis(5);

pub struct SimulatorSpawner {
    pub scan_result: ScanResult,
}

impl SpeakerSpawner for SimulatorSpawner {
    fn spawn(&self, target: Target, wake: Arc<dyn Fn() + Send + Sync>) -> (SpeakerHandle, Events) {
        speaker::spawn(target, Timing::default(), wake)
    }

    fn scan(&self) -> Receiver<ScanResult> {
        let (sender, receiver) = mpsc::channel();
        sender.send(self.scan_result.clone()).unwrap();
        receiver
    }
}

pub fn manual_for(simulator: &Simulator) -> ManualSpeaker {
    ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: simulator.address().port(),
        serial: None,
    }
}

pub fn settings_file(directory: &Path, manual: Vec<ManualSpeaker>) -> PathBuf {
    let path = directory.join(SETTINGS_FILE);
    let settings = Settings {
        manual,
        ..Settings::default()
    };
    settings::save(&settings, &path).unwrap();
    path
}

pub fn harness(settings_path: PathBuf, found: Vec<Found>) -> Harness<'static, NeumannApp> {
    harness_scanning(settings_path, Ok(found))
}

pub fn harness_scanning(
    settings_path: PathBuf,
    scan_result: ScanResult,
) -> Harness<'static, NeumannApp> {
    Harness::builder()
        .with_size(WINDOW_SIZE)
        .build_eframe(move |creation_context| {
            NeumannApp::new(
                &creation_context.egui_ctx,
                settings_path,
                Box::new(SimulatorSpawner { scan_result }),
            )
        })
}

pub fn wait_until(
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

/// Also steps the frames a newly opened dialog needs to settle: egui sizes a new modal in its
/// first frame and centres it in a later one, so a click right away lands beside the widget.
pub fn wait_for_label(harness: &mut Harness<'static, NeumannApp>, label: &str) {
    wait_until(harness, label, |harness| {
        harness.query_by_label(label).is_some()
    });
    harness.run_steps(DIALOG_SETTLE_FRAMES);
}

/// Scans, checks the named speakers in the dialog and adds them.
pub fn scan_and_add(harness: &mut Harness<'static, NeumannApp>, names: &[&str]) {
    harness.get_by_label(texts::SCAN.get()).click();
    for name in names {
        wait_for_label(harness, name);
        harness.get_by_label(name).click();
        harness.run_steps(2);
    }
    harness.get_by_label(texts::ADD.get()).click();
    harness.run_steps(2);
}

pub fn readout(value: f64) -> String {
    texts::readout(value, "dB SPL")
}

pub fn wait_for_level(harness: &mut Harness<'static, NeumannApp>, value: f64) {
    let label = readout(value);
    wait_until(harness, &label, |harness| {
        harness.query_by_label(&label).is_some()
    });
}

/// Requests the simulator must receive after a frame before "nothing was sent" can be
/// asserted: the worker runs commands before polls, so one request still in flight plus one
/// more mean anything the frame sent has arrived.
pub const BARRIER_REQUESTS: usize = 2;

/// Waits without drawing frames, so the window sends nothing new meanwhile.
pub fn barrier(simulator: &Simulator) {
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

pub fn level_sets(simulator: &Simulator) -> Vec<f64> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| request.pointer("/audio/out/level").and_then(Value::as_f64))
        .collect()
}

pub fn connected_speaker(
    model: SimModel,
) -> (Simulator, tempfile::TempDir, Harness<'static, NeumannApp>) {
    let simulator = Simulator::start(model);
    simulator.set_reply_delay(REPLY_DELAY);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut harness = harness(path, Vec::new());
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &simulator);
    (simulator, directory, harness)
}

/// Steps frames until the window holds every value the worker reads on connecting, which is
/// one read per catalog path of the model, in catalog order. The worker sends its next request
/// only after handing the previous reply to the window, so a request after the read of the
/// last path means all those values are in the channel; one more frame applies them.
pub fn wait_for_initial_read(harness: &mut Harness<'static, NeumannApp>, simulator: &Simulator) {
    let product = simulator
        .value(PRODUCT_PATH)
        .and_then(|product| product.as_str().map(str::to_owned))
        .expect("the simulator reports a product");
    let entries = Catalog::embedded().entries_for(&product);
    let last = entries.last().expect("the catalog has paths for the model");
    let pointer = format!("/{}", last.path);
    wait_until(harness, "the initial read", |_| {
        let requests = simulator.requests();
        let last_read = requests
            .iter()
            .position(|request| request.pointer(&pointer).is_some_and(Value::is_null));
        last_read.is_some_and(|index| requests.len() > index + 1)
    });
    harness.step();
}

pub fn readouts(harness: &Harness<'static, NeumannApp>) -> usize {
    harness.query_all_by_label(&readout(100.0)).count()
}

pub fn shows_one_speaker(harness: &Harness<'static, NeumannApp>) -> bool {
    readouts(harness) == 1 && harness.query_by_label(&texts::speaker_count(1)).is_some()
}

pub fn serials_remembered(path: &Path, count: usize) -> bool {
    let manual = settings::load(path).manual;
    manual.len() == count
        && manual
            .iter()
            .all(|entry| entry.serial.as_deref() == Some(SIMULATED_SERIAL))
}

pub fn device_name(model: SimModel) -> &'static str {
    match model {
        SimModel::Kh120Ii => KH120_DEVICE_NAME,
        SimModel::Kh750 => KH750_DEVICE_NAME,
    }
}

/// Clicks the strip's name and waits until the device panel shows its settings.
pub fn open_panel(harness: &mut Harness<'static, NeumannApp>, name: &str) {
    // The panel takes its size from the window it opens in.
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness.get_by_label(name).click();
    wait_until(harness, "the device panel", |harness| {
        harness.query_by_label(texts::ALL_VALUES.get()).is_some()
    });
    harness.run_steps(1);
}

pub const KH120_ID: &str = "serial:SIM0001234";
pub const KH750_ID: &str = "serial:SIM0007500";
pub const GROUP_NAME: &str = "Front";

/// A KH 120 II and a KH 750 in one group "Front", linking `linked`, both connected.
pub fn grouped(
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

pub fn sets(simulator: &Simulator, pointer: &str) -> Vec<Value> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| request.pointer(pointer).filter(|value| !value.is_null()))
        .cloned()
        .collect()
}

pub fn delay_sets(simulator: &Simulator) -> Vec<Value> {
    simulator
        .requests()
        .iter()
        .filter_map(|request| {
            request
                .pointer("/audio/out/delay")
                .filter(|value| !value.is_null())
        })
        .cloned()
        .collect()
}

/// The harness clock moves about half a second per frame, more than egui allows between the
/// two clicks of a double click, so the allowance is widened for the test.
pub fn double_click_at(harness: &mut Harness<'static, NeumannApp>, position: egui::Pos2) {
    harness
        .ctx
        .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
    harness.hover_at(position);
    for pressed in [true, false, true, false] {
        harness.event(egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run_steps(2);
}

pub fn around(center: egui::Pos2, radius: f32, degrees: f32) -> egui::Pos2 {
    let radians = degrees.to_radians();
    center + radius * egui::vec2(radians.sin(), -radians.cos())
}

/// Presses at `from` degrees on the knob's rim and moves clockwise to `to` without releasing.
pub fn turn_knob(
    harness: &mut Harness<'static, NeumannApp>,
    label: &str,
    from: f32,
    to: f32,
) -> egui::Pos2 {
    let center = harness
        .get_by_role_and_label(eframe::egui::accesskit::Role::Slider, label)
        .rect()
        .center();
    let radius = 14.0;
    harness.drag_at(around(center, radius, from));
    let steps = 6;
    for step in 1..=steps {
        let degrees = from + (to - from) * step as f32 / steps as f32;
        harness.hover_at(around(center, radius, degrees));
        harness.step();
    }
    around(center, radius, to)
}
