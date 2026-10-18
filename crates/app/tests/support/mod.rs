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

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::spawner::{ScanResult, SpeakerSpawner};
use neumann_control_app::texts;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::{self, ManualSpeaker, Settings, SETTINGS_FILE};
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::{self, Events, SpeakerHandle, Target, Timing};
use serde_json::Value;

pub const SIMULATED_SERIAL: &str = "SIM0001234";
pub const WAIT_LIMIT: Duration = Duration::from_secs(15);
// Paces the polling loop only; every outcome is decided by the condition.
pub const POLL_PAUSE: Duration = Duration::from_millis(10);
pub const WINDOW_SIZE: [f32; 2] = [900.0, 700.0];
/// Tall enough for a card with "Mehr" open, so every control in it can be clicked.
pub const EXPANDED_WINDOW_SIZE: [f32; 2] = [900.0, 1600.0];
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
        window: None,
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
    harness.get_by_label(texts::SCAN).click();
    for name in names {
        wait_for_label(harness, name);
        harness.get_by_label(name).click();
        harness.run_steps(2);
    }
    harness.get_by_label(texts::ADD).click();
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

pub fn connected_card(
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

pub fn has_one_card(harness: &Harness<'static, NeumannApp>) -> bool {
    readouts(harness) == 1 && harness.query_by_label(&texts::speaker_count(1)).is_some()
}

pub fn serials_remembered(path: &Path, count: usize) -> bool {
    let manual = settings::load(path).manual;
    manual.len() == count
        && manual
            .iter()
            .all(|entry| entry.serial.as_deref() == Some(SIMULATED_SERIAL))
}
