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
/// A stereo device's two channels. Beyond two, the positions depend on the device's channel
/// mask (7.1 puts the rear pair before the side pair, quad has no centre), which the app does
/// not read: those channels are numbered instead of named wrong.
const STEREO_NAMES: [&str; 2] = ["L", "R"];

pub struct SystemStrip<'a> {
    pub name: &'a str,
    /// Percent; `None` while the device cannot be read.
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub meters: &'a [MeterState],
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
            response.volume = moved.map(|moved| (volume + moved).clamp(0.0, 100.0));
            let names = channel_names(strip.meters.len());
            let channels: Vec<&str> = names.iter().map(String::as_str).collect();
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

fn channel_names(count: usize) -> Vec<String> {
    if count == STEREO_NAMES.len() {
        return STEREO_NAMES.iter().map(|name| (*name).to_owned()).collect();
    }
    (1..=count).map(|channel| channel.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stereo_channels_are_named() {
        assert_eq!(channel_names(2), vec!["L", "R"]);
        assert_eq!(channel_names(8)[4..], ["5", "6", "7", "8"]);
        assert_eq!(channel_names(1), vec!["1"]);
    }
}
