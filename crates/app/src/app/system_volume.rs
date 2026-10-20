//! The Windows volume strip on the app side: read once a frame while shown, the meters fed from
//! the peaks, and its gestures handed to Windows. Not part of the speakers' history.

use std::time::Duration;

use super::NeumannApp;
use crate::system_audio::peak_db;
use crate::texts;
use crate::views::system_strip::SystemStripResponse;

/// Windows' peak value is read every frame; this keeps frames coming for the meters.
const METER_REFRESH: Duration = Duration::from_millis(50);
const PERCENT: f64 = 100.0;

/// What the strip shows this frame; the meters stay in the app.
pub(super) struct SystemView {
    pub name: String,
    pub volume: Option<f64>,
    pub mute: Option<bool>,
}

impl NeumannApp {
    /// `None` while the strip is switched off.
    pub(super) fn read_system_volume(&mut self, now: f64) -> Option<SystemView> {
        let settings = &self.settings.system_audio;
        if !settings.shown {
            return None;
        }
        let device = settings.device.clone();
        let Some(state) = self.system_audio.state(device.as_deref()) else {
            self.system_meters.clear();
            return Some(SystemView {
                name: texts::SYSTEM_AUDIO_NAME.get().to_owned(),
                volume: None,
                mute: None,
            });
        };
        let floor = self.settings.desk.meter_floor.db();
        self.system_meters
            .resize(state.peaks.len(), Default::default());
        for (meter, peak) in self.system_meters.iter_mut().zip(&state.peaks) {
            meter.update(peak_db(*peak, floor), now);
        }
        Some(SystemView {
            name: state.name,
            volume: Some(f64::from(state.volume) * PERCENT),
            mute: Some(state.mute),
        })
    }

    pub(super) fn apply_system_volume(&mut self, response: SystemStripResponse) {
        let device = self.settings.system_audio.device.clone();
        if let Some(volume) = response.volume {
            let scalar = (volume / PERCENT) as f32;
            self.system_audio.set_volume(device.as_deref(), scalar);
        }
        if let Some(mute) = response.mute {
            self.system_audio.set_mute(device.as_deref(), mute);
        }
    }

    pub(super) fn keep_system_meters_moving(&self, context: &eframe::egui::Context) {
        if self.settings.system_audio.shown {
            context.request_repaint_after(METER_REFRESH);
        }
    }
}
