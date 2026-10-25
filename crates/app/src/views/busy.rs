//! The turning arc shown while something runs (a scan, a check, a full read).

use std::f64::consts::TAU;
use std::time::Duration;

use eframe::egui::accesskit::Role;
use eframe::egui::{emath::lerp, vec2, Pos2, Response, Sense, Shape, Stroke, Ui, WidgetInfo};

/// egui's own spinner asks for the next frame at once, which without vsync is several hundred a
/// second; a turning arc looks smooth at this rate.
const SPINNER_FRAME: Duration = Duration::from_millis(33);
const SPINNER_STROKE_WIDTH: f32 = 3.0;
/// Inset from the square so the stroke stays inside it.
const SPINNER_INSET: f32 = 2.0;
const SPINNER_MIN_POINTS: u32 = 8;
const SPINNER_MAX_POINTS: u32 = 128;
const SPINNER_SWEEP_DEGREES: f64 = 240.0;

/// The arc in a square of the interaction height, drawn like egui's spinner.
pub fn spinner(ui: &mut Ui) -> Response {
    let size = ui.style().spacing.interact_size.y;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    response.widget_info(|| WidgetInfo::new(Role::ProgressIndicator));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    ui.ctx().request_repaint_after(SPINNER_FRAME);
    let color = ui.visuals().strong_text_color();
    let radius = rect.height().min(rect.width()) * 0.5 - SPINNER_INSET;
    let point_count = (radius.round() as u32).clamp(SPINNER_MIN_POINTS, SPINNER_MAX_POINTS);
    let time = ui.input(|input| input.time);
    let start = time * TAU;
    let end = start + SPINNER_SWEEP_DEGREES.to_radians() * time.sin();
    let points: Vec<Pos2> = (0..point_count)
        .map(|index| {
            let angle = lerp(start..=end, f64::from(index) / f64::from(point_count));
            let (sin, cos) = angle.sin_cos();
            rect.center() + radius * vec2(cos as f32, sin as f32)
        })
        .collect();
    ui.painter().add(Shape::line(
        points,
        Stroke::new(SPINNER_STROKE_WIDTH, color),
    ));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::RequestRepaintInfo;
    use std::sync::{Arc, Mutex};

    /// Every repaint request of a settled pass that runs `contents`.
    fn requests(contents: impl Fn(&mut Ui)) -> Vec<Duration> {
        let context = eframe::egui::Context::default();
        let input = || eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                Pos2::ZERO,
                vec2(200.0, 100.0),
            )),
            ..Default::default()
        };
        // egui's first pass asks for a second one on its own.
        context.run_ui(input(), &contents).textures_delta.clear();
        let delays = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&delays);
        context.set_repaint_observer(move |info: RequestRepaintInfo| {
            recorded.lock().unwrap().push(info.delay)
        });
        context.run_ui(input(), &contents).textures_delta.clear();
        let delays = delays.lock().unwrap().clone();
        delays
    }

    fn immediate(delays: &[Duration]) -> usize {
        delays.iter().filter(|delay| delay.is_zero()).count()
    }

    #[test]
    fn the_spinner_asks_for_its_next_frame_later_not_at_once() {
        let turning = requests(|ui| {
            spinner(ui);
        });
        let nothing = requests(|_| {});
        assert!(turning.contains(&SPINNER_FRAME), "{turning:?}");
        assert_eq!(immediate(&turning), immediate(&nothing), "{turning:?}");
    }
}
