//! The EQ window of one speaker, an OS window of its own (embedded as an inner window where the
//! backend has only one): output and container choice, the graph and the band table.

use eframe::egui::{
    self, vec2, Color32, Context, Key, RichText, Ui, ViewportBuilder, ViewportClass, ViewportId,
};

use super::eq_graph::{self, Graph, GraphAction};
use super::eq_table::{self, Table, TableAction};
use crate::texts;

const EQ_WINDOW_SIZE: [f32; 2] = [900.0, 640.0];
const EQ_WINDOW_MIN_SIZE: [f32; 2] = [640.0, 480.0];
const LEGEND_SWATCH: f32 = 10.0;

pub struct EqView<'a> {
    pub viewport: ViewportId,
    pub title: String,
    /// The outputs to choose from; one output needs no choice.
    pub outputs: &'a [String],
    pub output: usize,
    /// The containers of the chosen output, with their curve colours.
    pub tabs: &'a [(String, Color32)],
    pub container: usize,
    pub graph: Graph<'a>,
    /// `None` while the container's bands are not (or not sensibly) reported.
    pub table: Option<Table<'a>>,
    /// Why nothing can be written right now, or why the last write was refused.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EqAction {
    Close,
    Output(usize),
    Container(usize),
    Graph(GraphAction),
    Table(TableAction),
}

pub fn show(context: &Context, view: &EqView) -> Vec<EqAction> {
    let builder = ViewportBuilder::default()
        .with_title(&view.title)
        .with_inner_size(EQ_WINDOW_SIZE)
        .with_min_inner_size(EQ_WINDOW_MIN_SIZE);
    context.show_viewport_immediate(view.viewport, builder, |ui, class| {
        let mut actions = match class {
            ViewportClass::EmbeddedWindow => contents(ui, view),
            _ => {
                egui::CentralPanel::default()
                    .show(ui, |ui| contents(ui, view))
                    .inner
            }
        };
        let close_requested = ui.input(|input| input.viewport().close_requested());
        // Escape in a field cancels the edit; only Escape with nothing being edited closes.
        let editing = ui.memory(|memory| memory.focused().is_some());
        let escape = !editing && ui.input(|input| input.key_pressed(Key::Escape));
        if close_requested || escape {
            actions.push(EqAction::Close);
        }
        actions
    })
}

fn contents(ui: &mut Ui, view: &EqView) -> Vec<EqAction> {
    let mut actions = Vec::new();
    if view.outputs.len() > 1 {
        ui.horizontal(|ui| {
            for (index, output) in view.outputs.iter().enumerate() {
                if ui.selectable_label(index == view.output, output).clicked() {
                    actions.push(EqAction::Output(index));
                }
            }
        });
    }
    ui.horizontal(|ui| {
        for (index, (label, color)) in view.tabs.iter().enumerate() {
            let (swatch, _) =
                ui.allocate_exact_size(vec2(LEGEND_SWATCH, LEGEND_SWATCH), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, *color);
            if ui
                .selectable_label(index == view.container, label)
                .clicked()
            {
                actions.push(EqAction::Container(index));
            }
        }
        ui.label(RichText::new(texts::EQ_SUM).strong());
    });
    actions.extend(
        eq_graph::show(ui, &view.graph)
            .into_iter()
            .map(EqAction::Graph),
    );
    ui.weak(texts::EQ_COMPUTED);
    if let Some(notice) = &view.notice {
        ui.colored_label(ui.visuals().warn_fg_color, notice);
    }
    ui.separator();
    match &view.table {
        Some(table) => actions.extend(eq_table::show(ui, table).into_iter().map(EqAction::Table)),
        None => {
            ui.weak(texts::EQ_NOT_REPORTED);
        }
    }
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        actions.push(EqAction::Close);
    }
    actions
}
