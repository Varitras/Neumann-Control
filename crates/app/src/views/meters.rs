//! Level meters drawn side by side, one per channel, each with its clip lamp and held peak.

use eframe::egui::{pos2, vec2, CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType};
use serde_json::Value;

use crate::meter::{fraction, MeterState};
use crate::texts;
use crate::ui_kit::theme::Tokens;

const METER_WIDTH: f32 = 8.0;
const CLIP_LAMP_RADIUS: f32 = 3.5;
const CLIP_LAMP_SPACE: f32 = 10.0;
const PEAK_LINE_WIDTH: f32 = 2.0;

/// `clips` is the speaker's clip list for the same channels, if it reported one; `group`
/// names the meters for screen readers.
pub fn show(
    ui: &mut Ui,
    group: &str,
    meters: &[MeterState],
    clips: Option<&Value>,
    height: f32,
    tokens: &Tokens,
) {
    let now = ui.input(|input| input.time);
    let clips = clips.and_then(Value::as_array);
    for (channel, meter) in meters.iter().enumerate() {
        let clipped = clips
            .and_then(|clips| clips.get(channel))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (rect, response) = ui.allocate_exact_size(vec2(METER_WIDTH, height), Sense::hover());
        let label = texts::meter_label(group, channel + 1);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, &label));
        let lamp_color = if clipped {
            tokens.danger
        } else {
            tokens.clip_off
        };
        let lamp_center = pos2(rect.center().x, rect.top() + CLIP_LAMP_RADIUS);
        let track = Rect::from_min_max(pos2(rect.left(), rect.top() + CLIP_LAMP_SPACE), rect.max);
        let painter = ui.painter();
        painter.circle_filled(lamp_center, CLIP_LAMP_RADIUS, lamp_color);
        painter.rect_filled(track, CornerRadius::ZERO, tokens.meter_track);
        let level_top = track.bottom() - track.height() * fraction(meter.shown(now));
        let level = Rect::from_min_max(pos2(track.left(), level_top), track.max);
        painter.rect_filled(level, CornerRadius::ZERO, tokens.meter_fill);
        let peak_y = track.bottom() - track.height() * fraction(meter.peak(now));
        painter.hline(
            track.x_range(),
            peak_y,
            Stroke::new(PEAK_LINE_WIDTH, tokens.text_dim),
        );
        if clipped {
            response.on_hover_text(texts::CLIP);
        }
    }
    if meters.iter().any(|meter| meter.is_moving(now)) {
        ui.ctx().request_repaint();
    }
}
