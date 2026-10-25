//! The strip with Windows' own volume of a playback device: its name, a fader in percent, mute
//! and a peak meter per channel. It only reports gestures; the app talks to Windows.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align, Layout, RichText, Ui};

use super::fader::{self, Orientation, FADER_HEIGHT};
use super::group_strip::{frame, mute_button};
use super::meters::{self, Meters};
use crate::groups::SharedLevel;
use crate::meter::MeterState;
use crate::texts;
use crate::ui_kit::theme::Tokens;

const PERCENT_RANGE: RangeInclusive<f64> = 0.0..=100.0;

pub struct SystemStrip<'a> {
    pub name: &'a str,
    /// Percent; `None` while the device cannot be read.
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub meters: &'a [MeterState],
    /// One name per meter, from the device's speaker positions.
    pub channels: &'a [String],
    pub floor: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SystemStripResponse {
    /// The volume to set, in percent.
    pub volume: Option<f64>,
    pub mute: Option<bool>,
}

pub fn show(ui: &mut Ui, strip: &SystemStrip) -> SystemStripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = SystemStripResponse::default();
    frame(ui, tokens.border, |ui| {
        ui.add(egui::Label::new(RichText::new(strip.name).strong()).truncate());
        let Some(volume) = strip.volume else {
            ui.weak(texts::SYSTEM_AUDIO_UNAVAILABLE);
            return;
        };
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let level = SharedLevel {
                reference: volume,
                reach: PERCENT_RANGE,
            };
            let label = texts::SYSTEM_VOLUME.get();
            let moved = fader::shared(
                ui,
                label,
                Some(&level),
                PERCENT_RANGE,
                Orientation::Vertical,
            );
            let (lowest, highest) = (*PERCENT_RANGE.start(), *PERCENT_RANGE.end());
            response.volume = moved.map(|moved| (volume + moved).clamp(lowest, highest));
            let channels: Vec<&str> = strip.channels.iter().map(String::as_str).collect();
            let group = Meters {
                name: texts::SYSTEM_METERS.get(),
                channels: &channels,
                levels: strip.meters,
                clips: None,
                played: &[],
            };
            meters::show(ui, &group, FADER_HEIGHT, strip.floor, &tokens);
        });
        ui.add(egui::Label::new(
            RichText::new(texts::percent(volume)).monospace(),
        ));
        response.mute = mute_button(ui, texts::SYSTEM_MUTE.get(), strip.mute);
    });
    response
}
