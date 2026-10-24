//! One speaker as a channel strip: name (opens the device panel), status LEDs, the elements of
//! its model's layout above and below the level fader with meters, readout and response time,
//! and the last message. The strip only reports gestures; `writes` decides what becomes a command.

use eframe::egui::{
    self, vec2, Align, Button, Color32, ComboBox, CornerRadius, CursorIcon, DragAndDrop, Label,
    Layout, PointerButton, Rect, Response, RichText, Role, Sense, Stroke, Ui, Vec2, WidgetInfo,
};
use neumann_control_core::catalog::{Catalog, Entry, Kind};
use neumann_control_core::settings::{DeskSettings, StripElement};
use neumann_control_core::speaker::Command;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::fader::{self, Orientation, Scale, FADER_HEIGHT};
use super::knob;
use super::layout_menu::{self, Offer};
use super::meters::{self, Meters};
use super::strip_drop::{self, Dragged, Geometry, Slot};
use crate::display::shown_value;
use crate::eq::has_eq;
use crate::fader::{output_level_range, WHEEL_STEP_DB};
use crate::gesture::on_grid;
use crate::input_source::input_source;
use crate::meter::MeterState;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::status_leds::{status_leds, ConnectionLed};
use crate::strip_layout::{self, Area, LayoutChange, StripLayout};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent};

pub const STRIP_WIDTH: f32 = 140.0;
/// The frame of every strip; the group strip draws the same.
pub(super) const STRIP_MARGIN: i8 = 6;
pub(super) const STRIP_RADIUS: u8 = 6;
const GRIP_ID: &str = "grip";
const GRIP_ROWS: usize = 3;
const GRIP_COLUMNS: usize = 2;
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.2;
const GRIP_WIDTH: f32 = GRIP_COLUMNS as f32 * GRIP_DOT_SPACING;
const GRIP_HEIGHT: f32 = GRIP_ROWS as f32 * GRIP_DOT_SPACING;
pub(super) const BORDER_WIDTH: f32 = 1.0;
const LED_RADIUS: f32 = 3.5;
const LED_SPACING: f32 = 6.0;
pub(super) const UNREACHABLE_OPACITY: f32 = 0.55;
const METER_GROUP_GAP: f32 = 4.0;
const METER_SPACING: f32 = 3.0;
const CAPTION_HEIGHT: f32 = 16.0;
const LEVEL_ENTRY_ID: &str = "level_entry";
const KNOBS_PER_ROW: usize = 2;
const DRAG_ID: &str = "strip_element_drag";
const GEOMETRY_ID: &str = "strip_geometry";
/// How much of the card colour lies over the dragged element at its landing place.
const LANDING_COVER: f32 = 0.6;

pub struct StripResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// The name or "Details" was clicked: the speaker's panel opens.
    pub open_panel: bool,
    /// The strip's area, where another strip can be dropped.
    pub rect: Rect,
    pub open_eq: bool,
    /// A change of the model's layout from an element's context menu.
    pub layout: Option<LayoutChange>,
}

/// How one area is drawn: what it holds as stored, whether it is being edited, and what could
/// still be added (none before the speaker reported its model).
struct Arrange<'a> {
    offers: Option<&'a [Offer]>,
    editing: bool,
    area: Area,
    stored: &'a [StripElement],
    catalog: &'a Catalog,
    /// The model the layout belongs to; a drag carries it to strips of the same model only.
    product: &'a str,
    /// The dragged element at its landing place, drawn faded.
    landing: Option<&'a StripElement>,
}

/// What the strip's areas asked for besides writes, and where its elements were drawn.
#[derive(Default)]
struct Wishes {
    open_eq: bool,
    layout: Option<LayoutChange>,
    slots: Vec<Slot>,
}

/// What the strip shows besides its fixed part, and whether it is being edited.
pub struct StripView<'a> {
    pub layout: &'a StripLayout,
    pub editing: bool,
}

pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
    view: StripView,
    group_color: Option<Color32>,
    member: &str,
) -> StripResponse {
    let StripView { layout, editing } = view;
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
    let mut wishes = Wishes::default();
    // While an element of this model is dragged over the strip, it is drawn where it would land.
    let geometry_id = ui.scope_id().with(GEOMETRY_ID);
    let dragged = editing
        .then(|| DragAndDrop::payload::<Dragged>(ui.ctx()))
        .flatten()
        .filter(|dragged| dragged.product == speaker.product());
    let pointer = ui.ctx().pointer_latest_pos();
    let previous = ui.data(|data| data.get_temp::<Geometry>(geometry_id));
    let landing = match (&dragged, pointer, &previous) {
        (Some(dragged), Some(pointer), Some(previous)) if previous.strip.contains(pointer) => Some(
            strip_drop::landing(previous, layout, &dragged.element, pointer),
        ),
        _ => None,
    };
    let preview = landing
        .as_ref()
        .and_then(|change| strip_layout::apply(layout, change))
        .map(|stored| StripLayout {
            above: stored.above,
            below: stored.below,
        });
    let shown = preview.as_ref().unwrap_or(layout);
    let landed = preview
        .as_ref()
        .and(dragged.as_deref())
        .map(|dragged| &dragged.element);
    let product = speaker.product().to_owned();
    // Only a speaker that reported its model can change that model's layout.
    let offers = speaker
        .identity
        .is_some()
        .then(|| layout_menu::offers(layout, speaker.product(), catalog));
    let offers = offers.as_deref();
    let arrange = |area: Area| Arrange {
        offers,
        editing,
        area,
        stored: match area {
            Area::Above => &shown.above,
            Area::Below => &shown.below,
        },
        catalog,
        product: &product,
        landing: landed,
    };
    let mut fader = Rect::NOTHING;
    let inner_width = STRIP_WIDTH - 2.0 * (f32::from(STRIP_MARGIN) + BORDER_WIDTH);
    let frame = egui::Frame::new()
        .fill(tokens.card)
        // A member of a group is framed in the group's colour.
        .stroke(Stroke::new(
            BORDER_WIDTH,
            group_color.unwrap_or(tokens.border),
        ))
        .corner_radius(CornerRadius::same(STRIP_RADIUS))
        .inner_margin(STRIP_MARGIN)
        .show(ui, |ui| {
            // The desk lays strips out in a row; the content of one strip stacks.
            ui.vertical(|ui| {
                ui.set_width(inner_width);
                if speaker.status == Status::Unreachable {
                    ui.multiply_opacity(UNREACHABLE_OPACITY);
                }
                let open_panel = ui
                    .horizontal(|ui| {
                        grip(ui, member, &tokens);
                        name(ui, speaker)
                    })
                    .inner;
                leds(ui, speaker, &tokens);
                elements(
                    ui,
                    speaker,
                    &arrange(Area::Above),
                    &mut commands,
                    &mut wishes,
                );
                // While editing the fader is as still as the other controls.
                fader = ui
                    .add_enabled_ui(!editing, |ui| {
                        level(ui, speaker, catalog, desk, &mut commands, &tokens);
                    })
                    .response
                    .rect;
                if desk.show_response_times {
                    response_time(ui, speaker);
                }
                elements(
                    ui,
                    speaker,
                    &arrange(Area::Below),
                    &mut commands,
                    &mut wishes,
                );
                if let (true, Some(offers)) = (editing, offers) {
                    edit_controls(ui, speaker.product(), offers, &mut wishes);
                }
                message(ui, speaker, &tokens);
                let details = ui.button(texts::DETAILS).clicked();
                open_panel || details
            })
            .inner
        });
    if landing.is_some() && ui.input(|input| input.pointer.primary_released()) {
        wishes.layout = landing;
        DragAndDrop::clear_payload(ui.ctx());
    }
    if editing {
        let geometry = Geometry {
            slots: wishes.slots,
            fader,
            strip: frame.response.rect,
        };
        ui.data_mut(|data| data.insert_temp(geometry_id, geometry));
    }
    StripResponse {
        commands,
        open_panel: frame.inner,
        rect: frame.response.rect,
        open_eq: wishes.open_eq,
        layout: wishes.layout,
    }
}

/// The handle a strip is dragged by to another place on the desk; the payload is the speaker's
/// identity in the saved order.
fn grip(ui: &mut Ui, member: &str, tokens: &Tokens) {
    let id = ui.scope_id().with(GRIP_ID);
    let response = ui
        .dnd_drag_source(id, member.to_owned(), |ui| {
            let size = vec2(GRIP_WIDTH, GRIP_HEIGHT);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            for row in 0..GRIP_ROWS {
                for column in 0..GRIP_COLUMNS {
                    let center = rect.left_top()
                        + vec2(
                            GRIP_DOT_SPACING * (column as f32 + 0.5),
                            GRIP_DOT_SPACING * (row as f32 + 0.5),
                        );
                    ui.painter()
                        .circle_filled(center, GRIP_DOT_RADIUS, tokens.text_dim);
                }
            }
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, texts::MOVE_STRIP));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// True when clicked: the name opens the speaker's panel.
pub(super) fn name(ui: &mut Ui, speaker: &SpeakerState) -> bool {
    let text = RichText::new(speaker.display_name()).strong();
    ui.add(Button::new(text).frame(false).truncate()).clicked()
}

pub(super) fn leds(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    let state = status_leds(speaker);
    let connection = match state.connection {
        ConnectionLed::Connected => tokens.connected,
        ConnectionLed::Connecting => tokens.connecting,
        ConnectionLed::Off => tokens.unreachable,
    };
    let lit = |on: bool, color: Color32| if on { color } else { tokens.clip_off };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = LED_SPACING;
        led(ui, connection, texts::LED_CONNECTION.get());
        led(ui, lit(state.clip, tokens.danger), texts::LED_CLIP.get());
        led(
            ui,
            lit(state.warning, tokens.connecting),
            texts::LED_WARNING.get(),
        );
        led(ui, lit(state.local, tokens.accent), texts::LED_LOCAL.get());
    });
}

fn led(ui: &mut Ui, color: Color32, text: &str) {
    let size = Vec2::splat(2.0 * LED_RADIUS);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().circle_filled(rect.center(), LED_RADIUS, color);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    response.on_hover_text(text);
}

/// One area of the layout. Neighbours of one kind share a row: knobs two by two, buttons
/// wrapped; a select or a display takes the whole width. Paths the speaker lacks are skipped.
fn elements(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    arrange: &Arrange,
    commands: &mut Vec<Command>,
    wishes: &mut Wishes,
) {
    let catalog = arrange.catalog;
    let entries = catalog.entries_for(speaker.product());
    let drawn: Vec<Drawn> = arrange
        .stored
        .iter()
        .filter_map(|element| match element {
            StripElement::Eq => has_eq(catalog, speaker).then_some(Drawn::Eq),
            StripElement::Path(path) => entries
                .iter()
                .copied()
                .find(|entry| entry.path == *path)
                .filter(|_| !speaker.absent.contains(path))
                .map(Drawn::Entry),
        })
        .collect();
    for run in drawn.chunk_by(|left, right| left.row() == right.row()) {
        match run[0].row() {
            Row::Knobs => knob_rows(ui, speaker, arrange, run, commands, wishes),
            Row::Buttons => button_row(ui, speaker, arrange, run, commands, wishes),
            Row::Select | Row::Display => {
                for drawn in run {
                    element_cell(ui, drawn.element(), arrange, wishes, |ui| {
                        match drawn.entry() {
                            Some(entry) if entry.kind == Kind::Select => {
                                select(ui, speaker, catalog, entry, commands);
                            }
                            Some(entry) => display(ui, speaker, entry),
                            None => {}
                        }
                    });
                }
            }
        }
    }
}

/// Draws one element with its context menu (which also hides it). While editing it is a still
/// control, dragged as a whole with the primary button.
fn element_cell(
    ui: &mut Ui,
    element: StripElement,
    arrange: &Arrange,
    wishes: &mut Wishes,
    draw: impl FnOnce(&mut Ui),
) {
    let Some(offers) = arrange.offers else {
        ui.add_enabled_ui(!arrange.editing, draw);
        return;
    };
    let rect = match arrange.editing {
        false => ui.scope(draw).response.rect,
        true => {
            let rect = ui.scope(|ui| ui.add_enabled_ui(false, draw)).response.rect;
            arrange_while_editing(ui, rect, &element, arrange, wishes);
            rect
        }
    };
    element_menu(ui, rect, &element, offers, wishes);
}

/// Like `element_cell` for a button in a wrapped row. It gets no child area of its own: one would
/// only get the rest of the line and wrap the button's text instead of the row. `draw` is told
/// whether the button may be used and returns its response.
fn inline_cell(
    ui: &mut Ui,
    element: StripElement,
    arrange: &Arrange,
    wishes: &mut Wishes,
    draw: impl FnOnce(&mut Ui, bool) -> Response,
) {
    let response = draw(ui, !arrange.editing);
    let Some(offers) = arrange.offers else {
        return;
    };
    if arrange.editing {
        arrange_while_editing(ui, response.rect, &element, arrange, wishes);
    }
    element_menu(ui, response.rect, &element, offers, wishes);
}

fn element_menu(
    ui: &mut Ui,
    rect: Rect,
    element: &StripElement,
    offers: &[Offer],
    wishes: &mut Wishes,
) {
    if let Some(change) = layout_menu::element_menu(ui, rect, element, offers) {
        wishes.layout = Some(change);
    }
}

/// While editing: notes where the element is (for the next frame's landing), fades it when it
/// is the dragged one shown at its landing place, and lets it be dragged.
fn arrange_while_editing(
    ui: &mut Ui,
    rect: Rect,
    element: &StripElement,
    arrange: &Arrange,
    wishes: &mut Wishes,
) {
    wishes.slots.push(Slot {
        area: arrange.area,
        element: element.clone(),
        rect,
    });
    if arrange.landing == Some(element) {
        let cover = Tokens::current(ui.ctx()).card.gamma_multiply(LANDING_COVER);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(STRIP_RADIUS), cover);
    }
    drag_handle(ui, rect, element, arrange.product);
}

/// Lies over a still element while editing. Only the primary button drags: the secondary one
/// opens the element's menu, and egui would start a drag for it as well.
fn drag_handle(ui: &mut Ui, rect: Rect, element: &StripElement, product: &str) {
    let drag_id = ui
        .scope_id()
        .with((DRAG_ID, layout_menu::menu_key(element)));
    let handle = ui.interact(rect, drag_id, Sense::drag());
    if handle.drag_started_by(PointerButton::Primary) {
        let dragged = Dragged {
            product: product.to_owned(),
            element: element.clone(),
        };
        DragAndDrop::set_payload(ui.ctx(), dragged);
    }
    if handle.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
}

/// Under the strip while editing: whose layout it is, "+ Add" and "Default".
fn edit_controls(ui: &mut Ui, model: &str, offers: &[Offer], wishes: &mut Wishes) {
    ui.label(RichText::new(texts::applies_to_every(model)).small().weak());
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(texts::ADD_ELEMENT, |ui| {
            for offer in offers {
                if ui.button(&offer.label).clicked() {
                    let added = LayoutChange::Add(offer.element.clone(), Area::Below);
                    wishes.layout = Some(added);
                }
            }
        });
        if ui.button(texts::DEFAULT_LAYOUT).clicked() {
            wishes.layout = Some(LayoutChange::Reset);
        }
    });
}

#[derive(Clone, Copy)]
enum Drawn<'a> {
    Entry(&'a Entry),
    Eq,
}

#[derive(Clone, Copy, PartialEq)]
enum Row {
    Knobs,
    Buttons,
    Select,
    Display,
}

impl<'a> Drawn<'a> {
    fn entry(&self) -> Option<&'a Entry> {
        match self {
            Drawn::Entry(entry) => Some(entry),
            Drawn::Eq => None,
        }
    }

    fn element(&self) -> StripElement {
        match self {
            Drawn::Entry(entry) => StripElement::Path(entry.path.clone()),
            Drawn::Eq => StripElement::Eq,
        }
    }

    fn row(&self) -> Row {
        let Some(entry) = self.entry() else {
            return Row::Buttons;
        };
        match entry.kind {
            Kind::Number => Row::Knobs,
            Kind::Switch => Row::Buttons,
            Kind::Select => Row::Select,
            _ => Row::Display,
        }
    }
}

fn knob_rows(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    arrange: &Arrange,
    run: &[Drawn],
    commands: &mut Vec<Command>,
    wishes: &mut Wishes,
) {
    let catalog = arrange.catalog;
    for pair in run.chunks(KNOBS_PER_ROW) {
        ui.columns(KNOBS_PER_ROW, |columns| {
            for (column, entry) in columns.iter_mut().zip(pair.iter().filter_map(Drawn::entry)) {
                let element = StripElement::Path(entry.path.clone());
                element_cell(column, element, arrange, wishes, |ui| {
                    // Named as in the device panel, so one setting has one name.
                    let label = entry.label(texts::language());
                    let sent = knob::for_entry(
                        ui,
                        speaker,
                        catalog,
                        entry,
                        label,
                        true,
                        knob::KNOB_DIAMETER,
                    );
                    commands.extend(sent);
                });
            }
        });
    }
}

fn button_row(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    arrange: &Arrange,
    run: &[Drawn],
    commands: &mut Vec<Command>,
    wishes: &mut Wishes,
) {
    let catalog = arrange.catalog;
    let mut open_eq = false;
    ui.horizontal_wrapped(|ui| {
        for drawn in run {
            inline_cell(
                ui,
                drawn.element(),
                arrange,
                wishes,
                |ui, usable| match drawn {
                    Drawn::Eq => {
                        let eq = ui.add_enabled(usable, Button::new(texts::EQ));
                        open_eq |= eq.clicked();
                        eq
                    }
                    Drawn::Entry(entry) => {
                        let text = button_text(entry);
                        let path = entry.path.clone();
                        toggle(ui, speaker, catalog, commands, path, text, usable)
                    }
                },
            );
        }
    });
    wishes.open_eq |= open_eq;
}

/// The name from the panel, but some keep a short one of the strip's: identify to fit beside
/// mute, and the KH 750's three mutes, which the panel tells apart by their headings only.
pub(super) fn button_text(entry: &Entry) -> &str {
    let short = [
        (paths::IDENTIFY, texts::IDENTIFY),
        (paths::MUTE_OUTPUT_A, texts::MUTE_OUTPUT_A),
        (paths::MUTE_OUTPUT_B, texts::MUTE_OUTPUT_B),
    ];
    short
        .into_iter()
        .find(|(path, _)| entry.path == paths::path(path))
        .map_or_else(|| entry.label(texts::language()), |(_, text)| text.get())
}

fn select(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    entry: &Entry,
    commands: &mut Vec<Command>,
) {
    let current = speaker
        .values
        .get(&entry.path)
        .and_then(Value::as_str)
        .map(str::to_owned);
    let enabled = current.is_some() && writes::can_write(speaker, catalog, &entry.path);
    let current = current.unwrap_or_default();
    let caption = ui.label(RichText::new(entry.label(texts::language())).small());
    let mut chosen = None;
    ui.add_enabled_ui(enabled, |ui| {
        ComboBox::from_id_salt(entry.path.to_string())
            .selected_text(&current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for option in &entry.options {
                    let selected = *option == current;
                    if ui.selectable_label(selected, option).clicked() && !selected {
                        chosen = Some(option.clone());
                    }
                }
            })
            .response
            .labelled_by(caption.id);
    });
    if let Some(option) = chosen {
        let now = ui.input(|input| input.time);
        let intent = Intent::Write {
            path: entry.path.clone(),
            value: Value::String(option),
        };
        commands.extend(speaker.request(intent, catalog, now));
    }
}

fn display(ui: &mut Ui, speaker: &SpeakerState, entry: &Entry) {
    let shown = speaker
        .values
        .get(&entry.path)
        .map(|value| shown_value(entry, value))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.weak(RichText::new(entry.label(texts::language())).small());
        ui.weak(RichText::new(shown).small());
    });
}

fn level(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
    commands: &mut Vec<Command>,
    tokens: &Tokens,
) {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let device = speaker.values.get(&path).and_then(Value::as_f64);
    let unit = paths::catalog_entry(catalog, &path)
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default();
    // Top-aligned: the meter groups are taller than the fader (names, caption) and a centred
    // row would shift them against it and against each other.
    let shown = ui
        .with_layout(Layout::left_to_right(Align::Min), |ui| {
            let range = output_level_range();
            let scale = Scale {
                range,
                unit: &unit,
                orientation: Orientation::Vertical,
            };
            let shown = fader::show(ui, speaker, catalog, commands, device, scale);
            meters(ui, speaker, desk, tokens);
            shown
        })
        .inner;
    let readout = device
        .map(|_| texts::readout(shown, &unit))
        .unwrap_or_default();
    let label = Label::new(RichText::new(readout).monospace()).sense(Sense::click());
    let response = ui.add(label).on_hover_text(texts::CONTROL_HINT);
    let entry_id = response.id.with(LEVEL_ENTRY_ID);
    let open_now = response.double_clicked() && device.is_some();
    if let Some(level) = knob::value_entry(ui, entry_id, open_now, shown, None) {
        // Outside the range the typed value stays as typed, so the write policy refuses it
        // instead of the grid pulling it in ("120,2" is not 120).
        let level = if output_level_range().contains(&level) {
            on_grid(level, WHEEL_STEP_DB)
        } else {
            level
        };
        let now = ui.input(|input| input.time);
        commands.extend(speaker.request(Intent::Level(level), catalog, now));
    }
}

/// Input meters, then the output meters of models that report them (KH 750), set apart.
fn meters(ui: &mut Ui, speaker: &SpeakerState, desk: &DeskSettings, tokens: &Tokens) {
    let source = input_source(speaker);
    let caption = source.map_or(texts::METER_INPUT_SHORT, |source| source.caption);
    let played = source.map(|source| source.played).unwrap_or_default();
    let played: &[bool] = if source.is_some() { &played } else { &[] };
    let floor = desk.meter_floor.db();
    // Before its first reading a speaker has no meters; empty ones keep the strip's shape.
    let unread;
    let input_levels: &[MeterState] = if speaker.input_meters.is_empty() {
        unread = vec![MeterState::default(); texts::INPUT_CHANNELS.len()];
        &unread
    } else {
        &speaker.input_meters
    };
    let input = MeterGroup {
        with_scale: true,
        caption: caption.get(),
        meters: Meters {
            name: texts::INPUT_METERS.get(),
            channels: texts::INPUT_CHANNELS,
            levels: input_levels,
            clips: speaker.values.get(&paths::path(paths::INPUT_CLIP)),
            played,
        },
    };
    meter_group(ui, &input, floor, tokens);
    if speaker.output_meters.is_empty() {
        return;
    }
    ui.add_space(METER_GROUP_GAP);
    let output = MeterGroup {
        with_scale: false,
        caption: texts::METER_OUTPUT_SHORT.get(),
        meters: Meters {
            name: texts::OUTPUT_METERS.get(),
            channels: texts::OUTPUT_CHANNELS,
            levels: &speaker.output_meters,
            clips: speaker.values.get(&paths::path(paths::OUTPUT_CLIP)),
            played: &[],
        },
    };
    meter_group(ui, &output, floor, tokens);
}

struct MeterGroup<'a> {
    /// The first group carries the dB marks; the others share its heights.
    with_scale: bool,
    /// Under the meters, so input and output can be told apart at a glance.
    caption: &'a str,
    meters: Meters<'a>,
}

fn meter_group(ui: &mut Ui, group: &MeterGroup, floor: f64, tokens: &Tokens) {
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = METER_SPACING;
        if group.with_scale {
            meters::scale(ui, FADER_HEIGHT, floor, tokens);
        }
        // The caption belongs to the meters, not to the scale beside them.
        ui.vertical(|ui| {
            let meters = ui
                .horizontal(|ui| meters::show(ui, &group.meters, FADER_HEIGHT, floor, tokens))
                .inner;
            // Centred on the meters but never wrapped: "Analog" is wider than two meters.
            let caption = RichText::new(group.caption).small().weak();
            let width = ui
                .painter()
                .layout_no_wrap(caption.text().to_owned(), small_font(ui), tokens.text_dim)
                .size()
                .x;
            let size = vec2(meters.width(), CAPTION_HEIGHT);
            let (reserved, _) = ui.allocate_exact_size(size, Sense::hover());
            let row = Rect::from_center_size(
                egui::pos2(meters.center().x, reserved.center().y),
                vec2(width.max(meters.width()), CAPTION_HEIGHT),
            );
            ui.put(row, Label::new(caption).extend());
        });
    });
}

fn response_time(ui: &mut Ui, speaker: &SpeakerState) {
    let text = speaker
        .response_time
        .map(|time| texts::response_time(time.as_millis()))
        .unwrap_or_default();
    ui.add(Label::new(RichText::new(text).small().weak().monospace()));
}

/// A button showing a switch value of the speaker; a click asks for the opposite.
pub(super) fn toggle(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    path: SscPath,
    text: &str,
    usable: bool,
) -> Response {
    let current = speaker.values.get(&path).and_then(Value::as_bool);
    let enabled = usable && current.is_some() && writes::can_write(speaker, catalog, &path);
    let button = Button::new(text).selected(current.unwrap_or(false));
    let response = ui.add_enabled(enabled, button);
    if response.clicked() {
        let now = ui.input(|input| input.time);
        commands.extend(speaker.request(Intent::Toggle(path), catalog, now));
    }
    response
}

fn message(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    if let Some(message) = &speaker.message {
        let text = texts::describe_message(message);
        ui.add(Label::new(RichText::new(text).small().color(tokens.danger)).wrap());
    }
}

fn small_font(ui: &Ui) -> egui::FontId {
    egui::TextStyle::Small.resolve(ui.style())
}
