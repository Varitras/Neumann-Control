//! How the app starts speaker workers and scans; tests swap in the simulator behind the trait.

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use eframe::egui::Context;
use neumann_control_core::discovery::{self, Found, SCAN_DURATION};
use neumann_control_core::speaker::{self, Events, SpeakerHandle, Target, Timing};

const SCAN_THREAD_NAME: &str = "scan";

pub type ScanResult = Result<Vec<Found>, String>;

pub trait SpeakerSpawner {
    /// Starts a worker for `target`; `wake` runs after each of its events.
    fn spawn(&self, target: Target, wake: Arc<dyn Fn() + Send + Sync>) -> (SpeakerHandle, Events);

    /// Starts a scan in the background; the receiver gets exactly one result.
    fn scan(&self) -> Receiver<ScanResult>;
}

/// Real workers with the default timing and a real mDNS scan.
pub struct CoreSpawner {
    context: Context,
}

impl CoreSpawner {
    /// The context is woken when a scan ends, as the speaker workers do through `wake`.
    pub fn new(context: Context) -> CoreSpawner {
        CoreSpawner { context }
    }
}

impl SpeakerSpawner for CoreSpawner {
    fn spawn(&self, target: Target, wake: Arc<dyn Fn() + Send + Sync>) -> (SpeakerHandle, Events) {
        speaker::spawn(target, Timing::default(), wake)
    }

    fn scan(&self) -> Receiver<ScanResult> {
        let (sender, receiver) = mpsc::channel();
        let context = self.context.clone();
        let started = thread::Builder::new()
            .name(SCAN_THREAD_NAME.to_owned())
            .spawn(move || {
                // The app may have closed meanwhile; then nobody waits for the result.
                let _ = sender.send(discovery::scan(SCAN_DURATION));
                context.request_repaint();
            });
        if let Err(error) = started {
            log::error!("scan thread could not be started: {error}");
        }
        receiver
    }
}
