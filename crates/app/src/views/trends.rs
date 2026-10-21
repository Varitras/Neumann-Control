//! The device panel's trends of this session: the temperature as a step line and the warnings
//! as a bar, over the last two hours; hovering shows how long ago and what was read.

use eframe::egui::{
    self, pos2, vec2, Color32, Rect, RichText, Role, Sense, Stroke, Ui, WidgetInfo,
};
use neumann_control_core::catalog::Conversion;

use crate::display::converted;
use crate::texts;
use crate::trend::{shown_span, whole_minutes, Trend, TrendPoint, TREND_WINDOW_SECONDS};
use crate::ui_kit::theme::Tokens;

const LINE_HEIGHT: f32 = 48.0;
const BAR_HEIGHT: f32 = 8.0;
const LINE_WIDTH: f32 = 1.5;
/// Keeps the lowest and highest reading off the frame.
const LINE_MARGIN: f32 = 6.0;
/// A temperature that stayed the same still gets a band of this many degrees to sit in.
const MIN_SPAN_DEGREES: f64 = 2.0;
pub struct Trends<'a> {
    /// Seconds of the UI clock, the right edge.
    pub now: f64,
    /// `None` for a model without a temperature.
    pub temperature: Option<TemperatureTrend<'a>>,
    pub warnings: &'a Trend<Vec<String>>,
}

pub struct TemperatureTrend<'a> {
    /// As reported (kelvin).
    pub trend: &'a Trend<f64>,
    pub conversion: Conversion,
    /// The catalog's name of the setting, as in the panel's rows.
    pub label: &'a str,
}

pub fn show(ui: &mut Ui, trends: &Trends) {
    let tokens = Tokens::current(ui.ctx());
    ui.strong(texts::trends_title(TREND_WINDOW_SECONDS));
    let temperature = trends
        .temperature
        .as_ref()
        .filter(|temperature| !temperature.trend.is_empty());
    if temperature.is_none() && trends.warnings.is_empty() {
        ui.weak(texts::NO_TREND_VALUES);
        return;
    }
    let first = [
        temperature.and_then(|temperature| temperature.trend.first_time()),
        trends.warnings.first_time(),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min);
    let span = shown_span(first, trends.now);
    let start = trends.now - span;
    if let Some(temperature) = temperature {
        temperature_line(ui, temperature, start, trends.now, &tokens);
    }
    warning_bar(ui, trends.warnings, start, trends.now, &tokens);
    ui.horizontal(|ui| {
        ui.small(RichText::new(texts::trend_start(span)).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.small(RichText::new(texts::TREND_NOW).weak());
        });
    });
}

/// Where a time lies on the trend's x axis.
struct Axis {
    rect: Rect,
    start: f64,
    now: f64,
}

impl Axis {
    fn x(&self, time: f64) -> f32 {
        let fraction = ((time - self.start) / (self.now - self.start)).clamp(0.0, 1.0);
        self.rect.left() + fraction as f32 * self.rect.width()
    }

    fn time(&self, x: f32) -> f64 {
        let fraction = f64::from((x - self.rect.left()) / self.rect.width());
        self.start + fraction.clamp(0.0, 1.0) * (self.now - self.start)
    }
}

/// Each point with its end: the next point's time, or now for the last.
fn spans<T>(points: &[&TrendPoint<T>], now: f64) -> Vec<(f64, f64)> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let end = points.get(index + 1).map_or(now, |next| next.time);
            (point.time, end)
        })
        .collect()
}

fn temperature_line(
    ui: &mut Ui,
    temperature: &TemperatureTrend,
    start: f64,
    now: f64,
    tokens: &Tokens,
) {
    let (trend, conversion) = (temperature.trend, temperature.conversion);
    let points: Vec<&TrendPoint<f64>> = trend.points().collect();
    let shown = |kelvin: f64| conversion.apply(kelvin);
    let reported: Vec<f64> = points.iter().filter_map(|point| point.value).collect();
    let low = reported.iter().copied().fold(f64::INFINITY, f64::min);
    let high = reported.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let middle = shown((low + high) / 2.0);
    // Kelvin to Celsius only shifts the scale, so the span is the same in either unit.
    let half_span = ((high - low) / 2.0).max(MIN_SPAN_DEGREES / 2.0);
    let current = trend
        .value_at(now)
        .map(|kelvin| converted(conversion, *kelvin));
    ui.horizontal(|ui| {
        ui.label(temperature.label);
        if high > low {
            let range =
                texts::trend_range(&converted(conversion, low), &converted(conversion, high));
            ui.weak(range);
        }
    });
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, LINE_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
    let axis = Axis { rect, start, now };
    let y = |value: f64| {
        let fraction = ((value - middle) / (2.0 * half_span) + 0.5) as f32;
        let inner = rect.shrink2(vec2(0.0, LINE_MARGIN));
        inner.bottom() - fraction * inner.height()
    };
    let stroke = Stroke::new(LINE_WIDTH, tokens.accent);
    let mut previous: Option<egui::Pos2> = None;
    for (point, (from, to)) in points.iter().zip(spans(&points, now)) {
        let Some(value) = point.value.map(shown) else {
            previous = None;
            continue;
        };
        let left = pos2(axis.x(from), y(value));
        let right = pos2(axis.x(to), y(value));
        if let Some(previous) = previous {
            painter.line_segment([previous, left], stroke);
        }
        painter.line_segment([left, right], stroke);
        previous = Some(right);
    }
    let label = texts::temperature_trend(current.as_deref().unwrap_or_default());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, &label));
    if let Some(pointer) = response.hover_pos() {
        let time = axis.time(pointer.x);
        let text = trend.value_at(time).map_or_else(
            || texts::NOT_READ.get().to_owned(),
            |kelvin| converted(conversion, *kelvin),
        );
        response.on_hover_text(texts::trend_hover(minutes_ago(now, time), &text));
    }
}

fn warning_bar(ui: &mut Ui, trend: &Trend<Vec<String>>, start: f64, now: f64, tokens: &Tokens) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, BAR_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    let axis = Axis { rect, start, now };
    let points: Vec<&TrendPoint<Vec<String>>> = trend.points().collect();
    for (point, (from, to)) in points.iter().zip(spans(&points, now)) {
        let color: Option<Color32> = match &point.value {
            Some(active) if !active.is_empty() => Some(tokens.danger),
            Some(_) => Some(tokens.clip_off),
            None => None,
        };
        let Some(color) = color else {
            continue;
        };
        let span = Rect::from_min_max(
            pos2(axis.x(from), rect.top()),
            pos2(axis.x(to), rect.bottom()),
        );
        painter.rect_filled(span, 0.0, color);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, texts::WARNING_TREND.get()));
    if let Some(pointer) = response.hover_pos() {
        let time = axis.time(pointer.x);
        let text = match trend.value_at(time) {
            Some(active) if active.is_empty() => texts::NO_WARNING.get().to_owned(),
            Some(active) => active.join(", "),
            None => texts::NOT_READ.get().to_owned(),
        };
        response.on_hover_text(texts::trend_hover(minutes_ago(now, time), &text));
    }
}

fn minutes_ago(now: f64, time: f64) -> u64 {
    whole_minutes(now - time)
}
