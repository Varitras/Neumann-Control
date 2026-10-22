//! Level meters drawn side by side, one per channel: segments in the colour of their zone
//! (lit up to the level, dimmed above it, so the eye always sees where red begins), the held
//! peak as a thin line, the held peak as a number below, and a clip lamp on top. The scale is the spread one from `meter`.

use eframe::egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Label, Rect, Response, RichText, Role,
    Sense, Stroke, Ui, Vec2, WidgetInfo,
};
use std::time::Duration;

use serde_json::Value;

use crate::meter::{
    level_at, position, zone, MeterState, Zone, HOT_FROM_DB, METER_CEILING_DB, OVER_FROM_DB,
};
use crate::texts;
use crate::ui_kit::theme::Tokens;

const METER_WIDTH: f32 = 14.0;
const CLIP_LAMP_RADIUS: f32 = 3.5;
const CLIP_LAMP_SPACE: f32 = 10.0;
/// One segment and the gap below it.
const SEGMENT_PITCH: f32 = 5.0;
const SEGMENT_GAP: f32 = 1.0;
const SEGMENT_RADIUS: u8 = 1;
/// How strongly a segment above the level still shows its zone colour.
const UNLIT_OPACITY: f32 = 0.22;
const SCALE_WIDTH: f32 = 24.0;
const SCALE_TICK_LENGTH: f32 = 3.0;
const SCALE_FONT_SIZE: f32 = 8.5;
const SCALE_MARKS_DB: [f64; 5] = [0.0, -6.0, -20.0, -60.0, -90.0];
const CHANNEL_NAME_HEIGHT: f32 = 12.0;
const CHANNEL_NAME_SIZE: f32 = 9.0;
const READOUT_HEIGHT: f32 = 12.0;
const PEAK_LINE_HEIGHT: f32 = 2.0;
const PLAYED_MARK_SPACE: f32 = 8.0;
const PLAYED_MARK_RADIUS: f32 = 2.0;
const BAR_WIDTH: f32 = 72.0;
const BAR_HEIGHT: f32 = 6.0;
const BAR_RADIUS: u8 = 2;
/// How often a falling meter is drawn anew; the monitor's own rate would keep the GPU busy for
/// a movement the eye does not resolve finer.
const METER_FRAME: Duration = Duration::from_millis(50);

/// One group of meters, side by side.
pub struct Meters<'a> {
    /// Names the meters for screen readers.
    pub name: &'a str,
    /// What the speaker calls each channel.
    pub channels: &'a [&'a str],
    pub levels: &'a [MeterState],
    /// The speaker's clip list for the same channels, if it reported one.
    pub clips: Option<&'a Value>,
    /// Which channels the speaker plays; empty when it does not say.
    pub played: &'a [bool],
}

/// Returns the area the meters themselves cover, without names that stick out beside them.
pub fn show(ui: &mut Ui, group: &Meters, height: f32, floor: f64, tokens: &Tokens) -> Rect {
    let mut covered = Rect::NOTHING;
    let (name_of_group, channels, meters) = (group.name, group.channels, group.levels);
    let now = ui.input(|input| input.time);
    let clips = group.clips.and_then(Value::as_array);
    let font = FontId::proportional(CHANNEL_NAME_SIZE);
    let painter = ui.painter().clone();
    let text_width = |text: String| {
        painter
            .layout_no_wrap(text, font.clone(), tokens.text_dim)
            .size()
            .x
    };
    // Digits differ in width ("−104" is wider than "−120"), so a column keeps its width whatever
    // the level only if it fits every readout the meter can show.
    let readout_width = readouts(floor)
        .map(|readout| text_width(texts::meter_readout(readout)))
        .fold(0.0, f32::max);
    for (channel, meter) in meters.iter().enumerate() {
        let clipped = clips
            .and_then(|clips| clips.get(channel))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // The channel's name goes below `height`, so the scale beside stays level with it; the
        // column is as wide as the name ("Sub"), with the meter in its middle.
        let name = texts::channel_name(channels, channel);
        let name_width = text_width(name.clone()).max(readout_width);
        let mark_space = if group.played.is_empty() {
            0.0
        } else {
            PLAYED_MARK_SPACE
        };
        let size = vec2(
            METER_WIDTH.max(name_width),
            height + CHANNEL_NAME_HEIGHT + READOUT_HEIGHT + mark_space,
        );
        let (cell, response) = ui.allocate_exact_size(size, Sense::hover());
        covered = covered.union(cell);
        let rect = Rect::from_center_size(
            pos2(cell.center().x, cell.top() + height / 2.0),
            vec2(METER_WIDTH, height),
        );
        let name_bottom = rect.bottom() + CHANNEL_NAME_HEIGHT;
        let name_rect = Rect::from_min_max(
            pos2(cell.left(), rect.bottom()),
            pos2(cell.right(), name_bottom),
        );
        let caption = Label::new(RichText::new(&name).size(CHANNEL_NAME_SIZE).weak());
        ui.put(name_rect, caption);
        let readout = meter.readout(now, floor);
        let readout_bottom = name_bottom + READOUT_HEIGHT;
        let readout_rect = Rect::from_min_max(
            pos2(cell.left(), name_bottom),
            pos2(cell.right(), readout_bottom),
        );
        let readout_text = RichText::new(texts::meter_readout(readout)).size(CHANNEL_NAME_SIZE);
        ui.put(readout_rect, Label::new(readout_text));
        if group.played.get(channel).copied().unwrap_or(false) {
            let mark = Rect::from_min_max(pos2(cell.left(), readout_bottom), cell.max);
            ui.put(mark, |ui: &mut Ui| played_mark(ui, tokens));
        }
        let label = texts::meter_label(name_of_group, &name);
        response.widget_info(|| WidgetInfo {
            value: readout.map(f64::from),
            ..WidgetInfo::labeled(Role::ProgressIndicator, true, &label)
        });
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
        ui.ctx().request_repaint_after(METER_FRAME);
    }
    // Before the first reading there are no meters; an empty area would be infinitely negative.
    if meters.is_empty() {
        return Rect::from_min_size(ui.cursor().min, Vec2::ZERO);
    }
    covered
}

/// Every readout a meter can show above `floor`, and none.
fn readouts(floor: f64) -> impl Iterator<Item = Option<i32>> {
    let lowest = floor.round() as i32;
    let highest = METER_CEILING_DB.round() as i32;
    std::iter::once(None).chain((lowest..=highest).map(Some))
}

/// The dot under a channel the speaker plays.
fn played_mark(ui: &mut Ui, tokens: &Tokens) -> Response {
    let size = Vec2::splat(2.0 * PLAYED_MARK_RADIUS);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), PLAYED_MARK_RADIUS, tokens.text);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, texts::PLAYED_CHANNEL));
    response.on_hover_text(texts::PLAYED_CHANNEL)
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
    let rects = segment_rects(track, ui.ctx().pixels_per_point());
    let count = rects.len().max(1);
    let lit = (level * count as f32).round() as usize;
    for (segment, rect) in rects.into_iter().enumerate() {
        let middle = (segment as f32 + 0.5) / count as f32;
        let color = zone_color(zone(level_at(middle, floor)), tokens);
        let color = if segment < lit {
            color
        } else {
            color.gamma_multiply(UNLIT_OPACITY)
        };
        painter.rect_filled(rect, CornerRadius::same(SEGMENT_RADIUS), color);
    }
    if let Some(line) = peak_line(track, level, peak) {
        let color = zone_color(zone(level_at(peak, floor)), tokens);
        // Round ends, so the line reads as a mark and not as a thin segment.
        let round = CornerRadius::same((PEAK_LINE_HEIGHT / 2.0) as u8);
        painter.rect_filled(line, round, color);
    }
}

/// The held peak as a thin line at its exact height, kept inside the track; none when the bar
/// reaches it.
fn peak_line(track: Rect, level: f32, peak: f32) -> Option<Rect> {
    if peak <= level {
        return None;
    }
    let center = track.bottom() - track.height() * peak;
    let top = (center - PEAK_LINE_HEIGHT / 2.0).max(track.top());
    let line = Rect::from_min_max(
        pos2(track.left(), top),
        pos2(track.right(), top + PEAK_LINE_HEIGHT),
    );
    Some(line)
}

/// The segments from the bottom up, each the same whole number of physical pixels high with the
/// same gap: sizes derived from the track's height would round to uneven pixels.
fn segment_rects(track: Rect, pixels_per_point: f32) -> Vec<Rect> {
    let to_points = |pixels: f32| pixels / pixels_per_point;
    let pitch = (SEGMENT_PITCH * pixels_per_point).round().max(2.0);
    let gap = (SEGMENT_GAP * pixels_per_point).round().max(1.0);
    let top = (track.top() * pixels_per_point).ceil();
    let bottom = (track.bottom() * pixels_per_point).floor();
    let left = to_points((track.left() * pixels_per_point).round());
    let right = to_points((track.right() * pixels_per_point).round());
    let count = ((bottom - top + gap) / pitch).floor().max(0.0) as usize;
    (0..count)
        .map(|segment| {
            let segment_bottom = bottom - segment as f32 * pitch;
            let segment_top = segment_bottom - (pitch - gap);
            Rect::from_min_max(
                pos2(left, to_points(segment_top)),
                pos2(right, to_points(segment_bottom)),
            )
        })
        .collect()
}

/// One level lying in a compact row: the zones side by side, lit up to `db` and dimmed above,
/// like the strip's meters.
pub fn bar(ui: &mut Ui, db: Option<f64>, floor: f64, label: &str, tokens: &Tokens) {
    let (rect, response) = ui.allocate_exact_size(vec2(BAR_WIDTH, BAR_HEIGHT), Sense::hover());
    let lit = db.map_or(0.0, |db| position(db, floor));
    let zones = [
        (Zone::Safe, floor, HOT_FROM_DB),
        (Zone::Hot, HOT_FROM_DB, OVER_FROM_DB),
        (Zone::Over, OVER_FROM_DB, METER_CEILING_DB),
    ];
    let painter = ui.painter();
    let x = |at: f32| rect.left() + at * rect.width();
    for (zone, from, to) in zones {
        let (start, end) = (position(from, floor), position(to, floor));
        let color = zone_color(zone, tokens);
        let whole = Rect::from_x_y_ranges(x(start)..=x(end), rect.y_range());
        painter.rect_filled(
            whole,
            CornerRadius::same(BAR_RADIUS),
            color.gamma_multiply(UNLIT_OPACITY),
        );
        let shown = Rect::from_x_y_ranges(x(start)..=x(lit.clamp(start, end)), rect.y_range());
        painter.rect_filled(shown, CornerRadius::same(BAR_RADIUS), color);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::ProgressIndicator, true, label));
}

fn zone_color(zone: Zone, tokens: &Tokens) -> Color32 {
    match zone {
        Zone::Safe => tokens.meter_safe,
        Zone::Hot => tokens.meter_hot,
        Zone::Over => tokens.meter_over,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_equal_on_whole_pixels() {
        // A track whose edges fall between pixels, as in the strip.
        let track = Rect::from_min_max(pos2(10.3, 20.6), pos2(18.3, 207.4));
        let rects = segment_rects(track, 1.0);
        assert!(rects.len() > 30);
        let heights: Vec<f32> = rects.iter().map(Rect::height).collect();
        assert!(
            heights.iter().all(|height| *height == heights[0]),
            "{heights:?}"
        );
        for rect in &rects {
            assert_eq!(rect.top(), rect.top().round());
            assert_eq!(rect.bottom(), rect.bottom().round());
        }
        for pair in rects.windows(2) {
            assert_eq!(pair[0].top() - pair[1].bottom(), SEGMENT_GAP);
        }
    }

    #[test]
    fn peak_line_sits_at_the_peak_and_vanishes_at_the_bar() {
        let track = Rect::from_min_max(pos2(10.0, 0.0), pos2(24.0, 100.0));
        let line = peak_line(track, 0.2, 0.5).expect("a peak above the bar");
        assert_eq!(line.center().y, 50.0);
        assert_eq!(line.height(), PEAK_LINE_HEIGHT);
        assert_eq!(line.x_range(), track.x_range());
        assert_eq!(peak_line(track, 0.5, 0.5), None);
        assert_eq!(peak_line(track, 0.6, 0.5), None);
        let top = peak_line(track, 0.2, 1.0).expect("a peak at the top");
        assert_eq!(top.top(), track.top());
    }

    #[test]
    fn segments_follow_the_pixel_grid_at_other_scales() {
        let track = Rect::from_min_max(pos2(10.0, 20.0), pos2(18.0, 200.0));
        let pixels_per_point = 1.25;
        for rect in segment_rects(track, pixels_per_point) {
            let top = rect.top() * pixels_per_point;
            assert!((top - top.round()).abs() < 1e-3, "{rect:?}");
        }
    }
}
