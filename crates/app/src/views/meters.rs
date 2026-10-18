//! Level meters drawn side by side, one per channel: segments in the colour of their zone
//! (lit up to the level, dimmed above it, so the eye always sees where red begins), the held
//! peak as one lit segment, and a clip lamp on top. The scale is the spread one from `meter`.

use eframe::egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, WidgetInfo,
    WidgetType,
};
use serde_json::Value;

use crate::meter::{level_at, position, zone, MeterState, Zone};
use crate::texts;
use crate::ui_kit::theme::Tokens;

const METER_WIDTH: f32 = 8.0;
const CLIP_LAMP_RADIUS: f32 = 3.5;
const CLIP_LAMP_SPACE: f32 = 10.0;
/// One segment and the gap below it.
const SEGMENT_PITCH: f32 = 5.0;
const SEGMENT_GAP: f32 = 1.0;
/// How strongly a segment above the level still shows its zone colour.
const UNLIT_OPACITY: f32 = 0.22;
const SCALE_WIDTH: f32 = 18.0;
const SCALE_TICK_LENGTH: f32 = 3.0;
const SCALE_FONT_SIZE: f32 = 8.5;
const SCALE_MARKS_DB: [f64; 5] = [0.0, -6.0, -20.0, -60.0, -90.0];

/// `clips` is the speaker's clip list for the same channels, if it reported one; `group`
/// names the meters for screen readers.
pub fn show(
    ui: &mut Ui,
    group: &str,
    meters: &[MeterState],
    clips: Option<&Value>,
    height: f32,
    floor: f64,
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
        let painter = ui.painter();
        let lamp_center = pos2(rect.center().x, rect.top() + CLIP_LAMP_RADIUS);
        painter.circle_filled(lamp_center, CLIP_LAMP_RADIUS, lamp_color);
        let track = Rect::from_min_max(pos2(rect.left(), rect.top() + CLIP_LAMP_SPACE), rect.max);
        let level = position(meter.shown(now), floor);
        let peak = position(meter.peak(now), floor);
        paint_segments(ui, track, level, peak, floor, tokens);
        if clipped {
            response.on_hover_text(texts::CLIP);
        }
    }
    if meters.iter().any(|meter| meter.is_moving(now)) {
        ui.ctx().request_repaint();
    }
}

/// dB marks beside the meters, at the same heights; marks below `floor` are left out.
pub fn scale(ui: &mut Ui, height: f32, floor: f64, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(SCALE_WIDTH, height), Sense::hover());
    let track = Rect::from_min_max(pos2(rect.left(), rect.top() + CLIP_LAMP_SPACE), rect.max);
    let painter = ui.painter();
    for mark in SCALE_MARKS_DB.into_iter().filter(|mark| *mark > floor) {
        let y = track.bottom() - track.height() * position(mark, floor);
        let tick = [
            pos2(track.right() - SCALE_TICK_LENGTH, y),
            pos2(track.right(), y),
        ];
        painter.line_segment(tick, Stroke::new(1.0, tokens.text_dim));
        painter.text(
            pos2(track.right() - SCALE_TICK_LENGTH - 1.0, y),
            Align2::RIGHT_CENTER,
            texts::scale_mark(mark),
            FontId::monospace(SCALE_FONT_SIZE),
            tokens.text_dim,
        );
    }
}

fn paint_segments(ui: &Ui, track: Rect, level: f32, peak: f32, floor: f64, tokens: &Tokens) {
    let painter = ui.painter();
    let count = (track.height() / SEGMENT_PITCH).floor().max(1.0) as usize;
    let pitch = track.height() / count as f32;
    let lit = (level * count as f32).round() as usize;
    let peak_segment = (peak * count as f32).ceil() as usize;
    for segment in 0..count {
        let middle = (segment as f32 + 0.5) / count as f32;
        let color = zone_color(zone(level_at(middle, floor)), tokens);
        let shown = segment < lit || segment + 1 == peak_segment;
        let color = if shown {
            color
        } else {
            color.gamma_multiply(UNLIT_OPACITY)
        };
        let bottom = track.bottom() - segment as f32 * pitch;
        let rect = Rect::from_min_max(
            pos2(track.left(), bottom - pitch + SEGMENT_GAP),
            pos2(track.right(), bottom),
        );
        painter.rect_filled(rect, CornerRadius::ZERO, color);
    }
}

fn zone_color(zone: Zone, tokens: &Tokens) -> Color32 {
    match zone {
        Zone::Safe => tokens.meter_safe,
        Zone::Hot => tokens.meter_hot,
        Zone::Over => tokens.meter_over,
    }
}
