//! The EQ graph: a log frequency axis, one curve per container, the sum, and a point per band
//! of the chosen container. A point is dragged for frequency and gain and turned with the
//! mouse wheel for Q; each gesture is handed out once, when it ends.

use std::ops::RangeInclusive;

use eframe::egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Id, Key, Mesh, Pos2, Rect, Role, Sense, Shape,
    Stroke, Ui, Vec2, WidgetInfo,
};

use super::wheel::notches_for;
use crate::eq::{snapped_frequency, GAIN_GRID_DB, Q_STEP};
use crate::gesture::{on_grid, GestureState};
use crate::texts;
use crate::ui_kit::eq_palette::ink_on;
use crate::ui_kit::theme::Tokens;
use neumann_control_core::catalog::{EQ_FREQUENCY_MAX_HZ, EQ_FREQUENCY_MIN_HZ};
use neumann_control_core::settings::BandCurves;

const GRAPH_HEIGHT: f32 = 300.0;
/// The dB axis shows at least ±24 dB and grows in 6 dB steps to fit the curves.
const DB_SPAN_MIN: f64 = 24.0;
const DB_GRID: f64 = 6.0;
const FREQUENCY_MARKS: [f64; 10] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
];
const LABEL_SIZE: f32 = 9.0;
const CURVE_WIDTH: f32 = 1.5;
const SUM_WIDTH: f32 = 2.5;
const DASH_LENGTH: f32 = 6.0;
const BAND_CURVE_WIDTH: f32 = 1.0;
const CHOSEN_CURVE_WIDTH: f32 = 1.5;
const BAND_CURVE_OPACITY: f32 = 0.65;
/// The chosen band's area between its curve and 0 dB.
const CHOSEN_AREA_OPACITY: f32 = 0.22;
const POINT_NUMBER_SIZE: f32 = 10.0;
const DASH_GAP: f32 = 4.0;
/// Big enough for a two-digit number inside.
const POINT_RADIUS: f32 = 7.0;
const SELECTED_RING: f32 = 3.0;
const POINT_GRAB: f32 = 14.0;
const DRAG_ID: &str = "eq_point_drag";
const WHEEL_ID: &str = "eq_point_wheel";

pub struct GraphCurve<'a> {
    pub color: Color32,
    pub db: &'a [f64],
}

/// The sum of an output, or of one input where the output has a crossover per input; the
/// second input's is drawn dashed.
pub struct SumCurve<'a> {
    pub db: &'a [f64],
    pub dashed: bool,
}

/// Another speaker's sum on the same output, drawn dashed for comparison.
pub struct ComparedCurve<'a> {
    pub color: Color32,
    pub db: &'a [f64],
}

#[derive(Clone, Copy, Debug)]
pub struct GraphPoint {
    /// The band's colour, for its point and its own curve.
    pub color: Color32,
    pub frequency: f64,
    pub gain: f64,
    pub boost: f64,
    pub q: f64,
    pub enabled: bool,
}

pub struct Graph<'a> {
    pub id: Id,
    pub frequencies: &'a [f64],
    pub curves: &'a [GraphCurve<'a>],
    pub sums: &'a [SumCurve<'a>],
    pub compared: &'a [ComparedCurve<'a>],
    /// Each band on its own, `None` for a band that is off; drawn thin in the band's colour.
    pub band_curves: &'a [Option<Vec<f64>>],
    pub band_curve_mode: BandCurves,
    pub points: &'a [GraphPoint],
    pub selected: usize,
    pub editable: bool,
    pub gain_range: RangeInclusive<f64>,
    pub q_range: RangeInclusive<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphAction {
    Select(usize),
    /// A drag ended; only the values it changed are given. A drag moves the filter (`boost`);
    /// the band's flat `gain` stays.
    Move {
        band: usize,
        frequency: Option<f64>,
        boost: Option<f64>,
    },
    Q {
        band: usize,
        q: f64,
    },
}

/// Maps frequency and level to the drawing area and back.
struct Axes {
    rect: Rect,
    db_span: f64,
}

impl Axes {
    fn x(&self, frequency: f64) -> f32 {
        let (low, high) = (EQ_FREQUENCY_MIN_HZ.ln(), EQ_FREQUENCY_MAX_HZ.ln());
        let share = (frequency
            .clamp(EQ_FREQUENCY_MIN_HZ, EQ_FREQUENCY_MAX_HZ)
            .ln()
            - low)
            / (high - low);
        self.rect.left() + share as f32 * self.rect.width()
    }

    fn y(&self, db: f64) -> f32 {
        let share = (db.clamp(-self.db_span, self.db_span) / self.db_span) as f32;
        self.rect.center().y - share * self.rect.height() / 2.0
    }

    fn frequency(&self, x: f32) -> f64 {
        let (low, high) = (EQ_FREQUENCY_MIN_HZ.ln(), EQ_FREQUENCY_MAX_HZ.ln());
        let share = f64::from(((x - self.rect.left()) / self.rect.width()).clamp(0.0, 1.0));
        (low + share * (high - low)).exp()
    }

    fn db(&self, y: f32) -> f64 {
        let share = f64::from((self.rect.center().y - y) / (self.rect.height() / 2.0));
        share.clamp(-1.0, 1.0) * self.db_span
    }
}

pub fn show(ui: &mut Ui, graph: &Graph) -> Vec<GraphAction> {
    let tokens = Tokens::current(ui.ctx());
    let size = vec2(ui.available_width(), GRAPH_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let axes = Axes {
        rect: rect.shrink2(vec2(28.0, 14.0)),
        db_span: db_span(graph),
    };
    paint_grid(ui, &axes, &tokens);
    for curve in graph.curves {
        paint_curve(
            ui,
            &axes,
            graph.frequencies,
            curve.db,
            Stroke::new(CURVE_WIDTH, curve.color),
        );
    }
    for curve in graph.compared {
        let stroke = Stroke::new(CURVE_WIDTH, curve.color);
        paint_dashed(ui, &axes, graph.frequencies, curve.db, stroke);
    }
    paint_band_curves(ui, graph, &axes);
    for sum in graph.sums {
        let stroke = Stroke::new(SUM_WIDTH, tokens.text);
        match sum.dashed {
            true => paint_dashed(ui, &axes, graph.frequencies, sum.db, stroke),
            false => paint_curve(ui, &axes, graph.frequencies, sum.db, stroke),
        }
    }
    let mut actions = Vec::new();
    // A band that is off changes nothing; it is switched on in the table.
    let enabled: Vec<(usize, &GraphPoint)> = graph
        .points
        .iter()
        .enumerate()
        .filter(|(_, point)| point.enabled)
        .collect();
    // Points can lie on one spot: the wheel turns only the one drawn last, which is on top.
    let wheeled = enabled
        .iter()
        .rev()
        .find(|(_, point)| !notches_for(ui, grab_rect(home(&axes, point))).is_empty())
        .map(|(band, _)| *band);
    for (band, point) in enabled {
        let takes_wheel = wheeled == Some(band);
        actions.extend(band_point(ui, graph, &axes, band, point, takes_wheel));
    }
    actions
}

/// Wide enough for every point and every boost of a curve. Cuts are not counted: a low or high
/// pass falls without end and would squeeze everything else flat; such a curve leaves the
/// graph at its bottom edge.
fn db_span(graph: &Graph) -> f64 {
    let own = graph.curves.iter().map(|curve| curve.db);
    let compared = graph.compared.iter().map(|curve| curve.db);
    let curves = own.chain(compared).flatten();
    let sums = graph.sums.iter().flat_map(|sum| sum.db);
    let boosts = curves.chain(sums).copied().filter(|db| *db > 0.0);
    let points = graph
        .points
        .iter()
        .filter(|point| point.enabled)
        .map(|point| (point.gain + point.boost).abs());
    let largest = boosts.chain(points).fold(0.0, f64::max);
    ((largest / DB_GRID).ceil() * DB_GRID).max(DB_SPAN_MIN)
}

/// A grid line every 6 dB up to ±24 dB, coarser on a wider axis so the labels stay apart.
fn db_grid(span: f64) -> f64 {
    DB_GRID * (span / DB_SPAN_MIN).ceil()
}

fn paint_grid(ui: &Ui, axes: &Axes, tokens: &Tokens) {
    let painter = ui.painter();
    let rect = axes.rect;
    let line = Stroke::new(1.0, tokens.border);
    let font = FontId::proportional(LABEL_SIZE);
    for mark in FREQUENCY_MARKS {
        let x = axes.x(mark);
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], line);
        let text = texts::frequency_mark(mark);
        let at = pos2(x, rect.bottom() + 2.0);
        painter.text(at, Align2::CENTER_TOP, text, font.clone(), tokens.text_dim);
    }
    let grid = db_grid(axes.db_span);
    let steps = (axes.db_span / grid) as i32;
    for step in -steps..=steps {
        let db = f64::from(step) * grid;
        let y = axes.y(db);
        let stroke = if step == 0 {
            Stroke::new(1.0, tokens.text_dim)
        } else {
            line
        };
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
        let at = pos2(rect.left() - 3.0, y);
        painter.text(
            at,
            Align2::RIGHT_CENTER,
            texts::scale_mark(db),
            font.clone(),
            tokens.text_dim,
        );
    }
}

fn paint_curve(ui: &Ui, axes: &Axes, frequencies: &[f64], db: &[f64], stroke: Stroke) {
    ui.painter()
        .add(Shape::line(curve_points(axes, frequencies, db), stroke));
}

fn paint_dashed(ui: &Ui, axes: &Axes, frequencies: &[f64], db: &[f64], stroke: Stroke) {
    let points = curve_points(axes, frequencies, db);
    ui.painter()
        .extend(Shape::dashed_line(&points, stroke, DASH_LENGTH, DASH_GAP));
}

fn curve_points(axes: &Axes, frequencies: &[f64], db: &[f64]) -> Vec<Pos2> {
    frequencies
        .iter()
        .zip(db)
        .map(|(frequency, db)| pos2(axes.x(*frequency), axes.y(*db)))
        .collect()
}

/// A point being dragged: it keeps the offset it had to the pointer when grabbed, so only the
/// axis the pointer moved along changes.
#[derive(Clone, Copy)]
struct PointDrag {
    offset: Vec2,
    at: Pos2,
}

/// Draws one band's point and hands out what its gesture chose.
fn band_point(
    ui: &mut Ui,
    graph: &Graph,
    axes: &Axes,
    band: usize,
    point: &GraphPoint,
    takes_wheel: bool,
) -> Vec<GraphAction> {
    let id = graph.id.with(band);
    let drag_id = id.with(DRAG_ID);
    // A gesture that ended while the point was not drawn (band switched off) leaves no state behind.
    let dragging = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
    let drag = ui
        .data(|data| data.get_temp::<PointDrag>(drag_id))
        .filter(|_| dragging);
    let home = home(axes, point);
    let center = drag.map_or(home, |drag| drag.at);
    let rect = grab_rect(center);
    let sense = if graph.editable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let response = ui.interact(rect, id, sense);
    let label = texts::eq_band(band + 1);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, graph.editable, &label));
    let mut actions = Vec::new();
    if response.clicked() || response.drag_started() {
        actions.push(GraphAction::Select(band));
    }
    let escaped = ui.input(|input| input.key_pressed(Key::Escape));
    if response.dragged() {
        let pointer = response.interact_pointer_pos().unwrap_or(center);
        // egui reports a drag only after the pointer moved a little; the offset is taken where
        // the button went down, else that first move would be lost.
        let pressed = ui
            .input(|input| input.pointer.press_origin())
            .unwrap_or(pointer);
        let mut state = drag.unwrap_or(PointDrag {
            offset: home - pressed,
            at: home,
        });
        state.at = (pointer + state.offset).clamp(axes.rect.min, axes.rect.max);
        ui.data_mut(|data| data.insert_temp(drag_id, state));
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.remove::<PointDrag>(drag_id));
        // egui ends a drag on Escape; that frame cancels the gesture instead of writing it.
        if let Some(end) = drag.filter(|_| !escaped) {
            actions.extend(moved(axes, graph, band, point, end.at));
        }
    }
    // Asked every frame: a wheel step falls due after the pointer may have left the point. A held
    // point ignores the wheel, as a held fader does.
    if graph.editable {
        let held = response.is_pointer_button_down_on() || response.dragged();
        let notches = match takes_wheel && !held {
            true => notches_for(ui, rect),
            false => Vec::new(),
        };
        actions.extend(wheel_q(ui, graph, id, band, point, &notches));
    }
    if graph.editable && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    paint_point(ui, graph, band, point, center);
    actions
}

/// Where a band's point stands when nothing moves it.
fn home(axes: &Axes, point: &GraphPoint) -> Pos2 {
    pos2(axes.x(point.frequency), axes.y(point.gain + point.boost))
}

fn grab_rect(center: Pos2) -> Rect {
    Rect::from_center_size(center, vec2(POINT_GRAB, POINT_GRAB))
}

fn moved(
    axes: &Axes,
    graph: &Graph,
    band: usize,
    point: &GraphPoint,
    end: Pos2,
) -> Option<GraphAction> {
    let frequency = snapped_frequency(axes.frequency(end.x));
    let range = &graph.gain_range;
    let boost = on_grid(axes.db(end.y) - point.gain, GAIN_GRID_DB);
    let boost = boost.clamp(*range.start(), *range.end());
    let frequency = (frequency != snapped_frequency(point.frequency)).then_some(frequency);
    let boost = (boost != on_grid(point.boost, GAIN_GRID_DB)).then_some(boost);
    let changed = frequency.is_some() || boost.is_some();
    changed.then_some(GraphAction::Move {
        band,
        frequency,
        boost,
    })
}

/// Wheel notches over a point step Q; the step goes out when the wheel goes quiet.
fn wheel_q(
    ui: &Ui,
    graph: &Graph,
    id: Id,
    band: usize,
    point: &GraphPoint,
    notches: &[f64],
) -> Option<GraphAction> {
    let wheel_id = id.with(WHEEL_ID);
    let now = ui.input(|input| input.time);
    let mut state = ui
        .data(|data| data.get_temp::<GestureState>(wheel_id))
        .unwrap_or_default();
    for notches in notches {
        state.on_wheel(*notches, Q_STEP, point.q, graph.q_range.clone(), now);
    }
    let due = state.due(now);
    if let Some(deadline) = state.deadline() {
        ui.ctx()
            .request_repaint_after_secs((deadline - now).max(0.0) as f32);
    }
    ui.data_mut(|data| data.insert_temp(wheel_id, state));
    due.map(|q| GraphAction::Q {
        band,
        q: on_grid(q, Q_STEP),
    })
}

fn paint_point(ui: &Ui, graph: &Graph, band: usize, point: &GraphPoint, center: Pos2) {
    let painter = ui.painter();
    if band == graph.selected {
        painter.circle_stroke(
            center,
            POINT_RADIUS + SELECTED_RING,
            Stroke::new(1.0, point.color),
        );
    }
    painter.circle_filled(center, POINT_RADIUS, point.color);
    painter.text(
        center,
        Align2::CENTER_CENTER,
        (band + 1).to_string(),
        FontId::proportional(POINT_NUMBER_SIZE),
        ink_on(point.color),
    );
}

/// Every band that is on (or only the chosen one and the one under the pointer); the chosen
/// band stronger, with its area to 0 dB.
fn paint_band_curves(ui: &Ui, graph: &Graph, axes: &Axes) {
    let hovered = hovered_band(ui, graph, axes);
    for (band, db) in graph.band_curves.iter().enumerate() {
        let Some(db) = db else {
            continue;
        };
        let Some(color) = graph.points.get(band).map(|point| point.color) else {
            continue;
        };
        let chosen = band == graph.selected;
        let shown = graph.band_curve_mode == BandCurves::All || chosen || hovered == Some(band);
        if !shown {
            continue;
        }
        if chosen {
            paint_area(
                ui,
                axes,
                graph.frequencies,
                db,
                color.gamma_multiply(CHOSEN_AREA_OPACITY),
            );
            paint_curve(
                ui,
                axes,
                graph.frequencies,
                db,
                Stroke::new(CHOSEN_CURVE_WIDTH, color),
            );
        } else {
            let faint = color.gamma_multiply(BAND_CURVE_OPACITY);
            paint_curve(
                ui,
                axes,
                graph.frequencies,
                db,
                Stroke::new(BAND_CURVE_WIDTH, faint),
            );
        }
    }
}

/// The band whose point is under the pointer, if any.
fn hovered_band(ui: &Ui, graph: &Graph, axes: &Axes) -> Option<usize> {
    let pointer = ui.input(|input| input.pointer.hover_pos())?;
    graph
        .points
        .iter()
        .enumerate()
        .filter(|(_, point)| point.enabled)
        .find(|(_, point)| {
            let home = pos2(axes.x(point.frequency), axes.y(point.gain + point.boost));
            home.distance(pointer) <= POINT_GRAB
        })
        .map(|(band, _)| band)
}

/// The area between a curve and 0 dB, as one strip of triangles.
fn paint_area(ui: &Ui, axes: &Axes, frequencies: &[f64], db: &[f64], color: Color32) {
    let zero = axes.y(0.0);
    let mut mesh = Mesh::default();
    for (place, (frequency, value)) in frequencies.iter().zip(db).enumerate() {
        let x = axes.x(*frequency);
        mesh.colored_vertex(pos2(x, zero), color);
        mesh.colored_vertex(pos2(x, axes.y(*value)), color);
        if place > 0 {
            let at = (2 * place) as u32;
            mesh.add_triangle(at - 2, at - 1, at);
            mesh.add_triangle(at - 1, at + 1, at);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
}
