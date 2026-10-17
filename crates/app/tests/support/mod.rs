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
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::{self, ManualSpeaker, Settings, SETTINGS_FILE};
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::{self, Event, SpeakerHandle, Target, Timing};
use serde_json::Value;

pub const SIMULATED_SERIAL: &str = "SIM0001234";
pub const WAIT_LIMIT: Duration = Duration::from_secs(15);
// Paces the polling loop only; every outcome is decided by the condition.
pub const POLL_PAUSE: Duration = Duration::from_millis(10);
pub const WINDOW_SIZE: [f32; 2] = [900.0, 700.0];
pub const LOOPBACK: &str = "::1";

pub struct SimulatorSpawner {
    pub found: Vec<Found>,
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
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut harness = harness(path, Vec::new());
    wait_for_level(&mut harness, 100.0);
    (simulator, directory, harness)
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
