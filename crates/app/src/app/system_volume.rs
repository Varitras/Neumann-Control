//! The Windows volume strip on the app side: read once a frame while shown, the meters fed from
//! the peaks, and its gestures handed to Windows. Not part of the speakers' history.

use std::time::Duration;

use super::NeumannApp;
use crate::meter::MeterState;
use crate::system_audio::{channel_names, peak_db};
use crate::texts;
use crate::views::system_strip::SystemStripResponse;

/// Windows' peak value is read every frame; this keeps frames coming for the meters.
const METER_REFRESH: Duration = Duration::from_millis(50);
/// In silence the peaks are read half as often: sound starting shows at most this late.
const SILENT_REFRESH: Duration = Duration::from_millis(100);
const PERCENT: f64 = 100.0;

/// What the strip shows this frame; the meters stay in the app.
pub(super) struct SystemView {
    pub name: String,
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    /// One name per meter.
    pub channels: Vec<String>,
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
                channels: Vec::new(),
            });
        };
        let floor = self.settings.desk.meter_floor.db();
        self.system_meters
            .resize(state.peaks.len(), Default::default());
        for (meter, peak) in self.system_meters.iter_mut().zip(&state.peaks) {
            meter.update(peak_db(*peak, floor), now);
        }
        Some(SystemView {
            channels: channel_names(state.channel_mask, state.peaks.len()),
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

    pub(super) fn keep_system_meters_moving(&self, context: &eframe::egui::Context, now: f64) {
        if self.settings.system_audio.shown {
            let floor = self.settings.desk.meter_floor.db();
            context.request_repaint_after(system_refresh(&self.system_meters, floor, now));
        }
    }
}

/// How soon Windows' peaks are read again: often while there is sound or a bar still falls.
fn system_refresh(meters: &[MeterState], floor: f64, now: f64) -> Duration {
    let sounding = meters
        .iter()
        .any(|meter| meter.level() > floor || meter.is_moving(now));
    if sounding {
        METER_REFRESH
    } else {
        SILENT_REFRESH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: f64 = -60.0;

    #[test]
    fn silence_is_read_less_often_than_sound() {
        let silent = [MeterState::default(); 2];
        assert_eq!(system_refresh(&silent, FLOOR, 5.0), SILENT_REFRESH);
        let mut sounding = silent;
        sounding[1].update(-20.0, 5.0);
        assert_eq!(system_refresh(&sounding, FLOOR, 5.0), METER_REFRESH);
        assert_eq!(
            system_refresh(&sounding, FLOOR, 60.0),
            METER_REFRESH,
            "a steady tone"
        );
        sounding[1].update(FLOOR, 60.0);
        assert_eq!(
            system_refresh(&sounding, FLOOR, 60.01),
            METER_REFRESH,
            "still falling"
        );
        assert_eq!(system_refresh(&sounding, FLOOR, 120.0), SILENT_REFRESH);
    }
}
