//! The windows of their own (device panel, EQ, all values, settings, presets) open where the
//! user last left one of their kind, for every speaker alike. Kept in the settings by kind, in
//! screen pixels like the main window.

use std::collections::BTreeMap;

use eframe::egui::{pos2, vec2, Context, Rect, Vec2, ViewportCommand, ViewportId};
use neumann_control_core::settings::WindowGeometry;

use super::{is_open, window_geometry, NeumannApp};
use crate::views::{Opening, WindowPlace};
use crate::window_fit::SCREEN_MARGIN;
use crate::window_placement::{fit_into, MonitorArea, WINDOW_MIN_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowKind {
    Panel,
    Eq,
    AllValues,
    Settings,
    Preset,
}

/// How far each further open window of a kind is set off from the one before, so they do not
/// cover each other exactly.
const CASCADE_STEP: Vec2 = Vec2::new(30.0, 30.0);

impl WindowKind {
    /// Where the kind's window is kept in the settings.
    fn key(self) -> &'static str {
        match self {
            WindowKind::Panel => "panel",
            WindowKind::Eq => "eq",
            WindowKind::AllValues => "all_values",
            WindowKind::Settings => "settings",
            WindowKind::Preset => "preset",
        }
    }
}

/// Keeps `geometry` as the place of `kind`. Places of earlier versions, one per speaker
/// (`"<kind> <speaker>"`), go: the kind's one place replaces them.
fn keep_place(
    windows: &mut BTreeMap<String, WindowGeometry>,
    kind: WindowKind,
    geometry: WindowGeometry,
) {
    let per_speaker = format!("{} ", kind.key());
    windows.retain(|key, _| !key.starts_with(&per_speaker));
    windows.insert(kind.key().to_owned(), geometry);
}

/// The `index`-th open window of a kind (from 0), set off from the others.
pub(super) fn cascaded(opening: Opening, index: usize) -> Opening {
    let Opening::At(place) = opening else {
        return opening;
    };
    let offset = CASCADE_STEP * index as f32;
    Opening::At(WindowPlace {
        position: place.position.map(|position| position + offset),
        ..place
    })
}

/// A window of `size` centred over the main window `main` (both in points), in screen pixels.
fn centred(main: Rect, size: Vec2, pixels_per_point: f32) -> WindowGeometry {
    let position = main.center() - size / 2.0;
    WindowGeometry {
        x: position.x * pixels_per_point,
        y: position.y * pixels_per_point,
        width: size.x * pixels_per_point,
        height: size.y * pixels_per_point,
        maximized: false,
    }
}

/// `geometry` on a monitor, kept `SCREEN_MARGIN` above its bottom: a window's foot (the panel's,
/// opened tall) once reached below the screen and the taskbar.
fn above_the_taskbar(
    geometry: WindowGeometry,
    monitors: &[MonitorArea],
    pixels_per_point: f32,
) -> WindowGeometry {
    let usable: Vec<MonitorArea> = monitors
        .iter()
        .map(|monitor| MonitorArea {
            height: monitor.height - SCREEN_MARGIN * pixels_per_point,
            ..*monitor
        })
        .collect();
    fit_into(geometry, &usable, least_size(pixels_per_point))
}

/// The least window size in screen pixels.
fn least_size(pixels_per_point: f32) -> (f32, f32) {
    (
        WINDOW_MIN_SIZE[0] * pixels_per_point,
        WINDOW_MIN_SIZE[1] * pixels_per_point,
    )
}

/// A window of `size` where the main window's place is unknown (Wayland tells no program where
/// its windows are): the system places it, but no larger than `monitor` (the main window's, in
/// points) less `SCREEN_MARGIN`. eframe keeps only the main window on its monitor.
fn unplaced(size: Vec2, monitor: Option<Vec2>) -> Option<WindowPlace> {
    let monitor = monitor?;
    Some(WindowPlace {
        position: None,
        size: size.min(monitor - Vec2::splat(SCREEN_MARGIN)),
        maximized: false,
    })
}

/// A saved geometry (screen pixels) as egui places a window (points).
pub(super) fn place_of(geometry: WindowGeometry, pixels_per_point: f32) -> WindowPlace {
    let points = |pixels: f32| pixels / pixels_per_point;
    WindowPlace {
        position: Some(pos2(points(geometry.x), points(geometry.y))),
        size: vec2(points(geometry.width), points(geometry.height)),
        maximized: geometry.maximized,
    }
}

impl NeumannApp {
    /// How the `index`-th open window of `kind` (from 0) on `viewport` is drawn: left alone
    /// while open, else where the last of its kind was left, else at `default_size` over the
    /// middle of the main window.
    pub(super) fn opening(
        &self,
        context: &Context,
        viewport: ViewportId,
        kind: WindowKind,
        index: usize,
        default_size: Vec2,
    ) -> Opening {
        let open = context.input_for(viewport, |input| is_open(input.viewport()));
        if open {
            return Opening::Open;
        }
        let monitor = context.input_for(ViewportId::ROOT, |input| input.viewport().monitor_size);
        let place = self
            .saved_place(context, kind)
            .or_else(|| self.centred_place(context, default_size))
            .or_else(|| unplaced(default_size, monitor));
        let opening = cascaded(place.map_or(Opening::Default, Opening::At), index);
        log::debug!(
            "{} window {index} opens {opening:?} on monitors {:?}",
            kind.key(),
            self.monitors
        );
        opening
    }

    /// Where the last `kind` window was left, if one was.
    fn saved_place(&self, context: &Context, kind: WindowKind) -> Option<WindowPlace> {
        let saved = self.settings.windows.get(kind.key())?;
        // On a monitor there is now: the saved one may have been unplugged since.
        Some(self.on_a_monitor(context, *saved))
    }

    /// A window of `size` over the middle of the main window, once the main window's place is
    /// known.
    fn centred_place(&self, context: &Context, size: Vec2) -> Option<WindowPlace> {
        let main = context.input_for(ViewportId::ROOT, |input| input.viewport().outer_rect)?;
        let pixels_per_point = context.pixels_per_point();
        let geometry = centred(main, size, pixels_per_point);
        let placed = above_the_taskbar(geometry, &self.monitors, pixels_per_point);
        Some(place_of(placed, pixels_per_point))
    }

    /// The main window to `geometry`, through egui as every size the fit asks for: set on the
    /// winit window directly, beside egui's commands, it came late or not at all under Wayland.
    pub(super) fn restore_window(&self, context: &Context, geometry: WindowGeometry) {
        let place = self.on_a_monitor(context, geometry);
        if let Some(position) = place.position {
            context.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        }
        context.send_viewport_cmd(ViewportCommand::InnerSize(place.size));
        context.send_viewport_cmd(ViewportCommand::Maximized(place.maximized));
    }

    /// `geometry` (screen pixels) moved and shrunk onto a monitor there is, as egui places it.
    fn on_a_monitor(&self, context: &Context, geometry: WindowGeometry) -> WindowPlace {
        let pixels_per_point = context.pixels_per_point();
        let fitted = fit_into(geometry, &self.monitors, least_size(pixels_per_point));
        place_of(fitted, pixels_per_point)
    }

    /// Notes where the `kind` window on `viewport` is now, as the place of its kind; a
    /// minimised or closed one keeps the last place.
    pub(super) fn remember_place(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        kind: WindowKind,
    ) {
        let last = self.settings.windows.get(kind.key()).copied();
        // The window's own scale: on another monitor it may differ from the main window's.
        let geometry = context.input_for(viewport, |input| {
            let size = input.viewport_rect().size();
            window_geometry(input.viewport(), size, input.pixels_per_point, last)
        });
        if let Some(geometry) = geometry {
            keep_place(&mut self.settings.windows, kind, geometry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::ViewportInfo;

    /// A tall window centred over a main window low on the screen ends above the taskbar (the
    /// panel's foot once reached below the screen).
    #[test]
    fn a_centred_window_ends_above_the_taskbar() {
        let screen = MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let main = Rect::from_min_size(pos2(100.0, 600.0), vec2(1200.0, 400.0));
        let geometry = centred(main, vec2(780.0, 760.0), 1.0);
        let placed = above_the_taskbar(geometry, &[screen], 1.0);
        assert!(placed.y >= 0.0, "{placed:?}");
        assert!(
            placed.y + placed.height <= 1080.0 - SCREEN_MARGIN,
            "{placed:?}"
        );
    }

    /// A window never left anywhere opens over the middle of the main window, not where the
    /// system puts new windows.
    #[test]
    fn a_window_without_a_place_is_centred_over_the_main_window() {
        let main = Rect::from_min_size(pos2(100.0, 50.0), vec2(600.0, 400.0));
        let geometry = centred(main, vec2(300.0, 200.0), 2.0);
        assert_eq!(
            geometry,
            WindowGeometry {
                x: 500.0,
                y: 300.0,
                width: 600.0,
                height: 400.0,
                maximized: false,
            }
        );
    }

    /// At 150 % a saved 300 pixels are 200 points: the window opens where it was, as large.
    #[test]
    fn a_saved_place_is_in_points_of_the_screen_scale() {
        let geometry = WindowGeometry {
            x: 150.0,
            y: 300.0,
            width: 900.0,
            height: 600.0,
            maximized: false,
        };
        let place = place_of(geometry, 1.5);
        assert_eq!(place.position, Some(pos2(100.0, 200.0)));
        assert_eq!(place.size, vec2(600.0, 400.0));
    }

    const GEOMETRY: WindowGeometry = WindowGeometry {
        x: 10.0,
        y: 20.0,
        width: 600.0,
        height: 400.0,
        maximized: false,
    };

    /// One place per kind for every speaker: the panel of another speaker used to open
    /// somewhere else, as only one speaker's place was known.
    #[test]
    fn a_kind_keeps_one_place_and_drops_the_old_per_speaker_ones() {
        let mut windows = BTreeMap::from([
            ("panel serial:SIM0001234".to_owned(), GEOMETRY),
            ("eq serial:SIM0001234".to_owned(), GEOMETRY),
        ]);
        let moved = WindowGeometry {
            x: 99.0,
            ..GEOMETRY
        };
        keep_place(&mut windows, WindowKind::Panel, moved);
        assert_eq!(windows.get("panel"), Some(&moved));
        assert!(!windows.contains_key("panel serial:SIM0001234"));
        assert!(windows.contains_key("eq serial:SIM0001234"));
    }

    /// Wayland tells no program where its windows are, so a window had no place to open at
    /// and came at its full default size: the EQ window (900×700) hung below a small screen.
    #[test]
    fn a_window_without_the_main_windows_place_still_fits_its_monitor() {
        let place = unplaced(vec2(900.0, 700.0), Some(vec2(1280.0, 720.0))).expect("a size");
        assert_eq!(place.position, None);
        assert_eq!(place.size, vec2(900.0, 720.0 - SCREEN_MARGIN));
        assert!(!place.maximized);
        assert_eq!(unplaced(vec2(900.0, 700.0), None), None);
    }

    /// Without a position (Wayland) a window counted as never open: it was given its first
    /// size again with every frame, and the user's resize was set back at once.
    #[test]
    fn a_window_is_open_once_it_exists_also_without_a_position() {
        let wayland = ViewportInfo {
            native_pixels_per_point: Some(1.25),
            ..ViewportInfo::default()
        };
        assert!(is_open(&wayland));
        assert!(!is_open(&ViewportInfo::default()));
    }

    #[test]
    fn a_further_open_window_is_set_off_from_the_first() {
        let place = place_of(GEOMETRY, 1.0);
        assert_eq!(cascaded(Opening::At(place), 0), Opening::At(place));
        let Opening::At(second) = cascaded(Opening::At(place), 2) else {
            panic!("still a place");
        };
        assert_eq!(second.position, Some(pos2(10.0, 20.0) + CASCADE_STEP * 2.0));
        assert_eq!(second.size, place.size);
        assert_eq!(cascaded(Opening::Open, 2), Opening::Open);
    }
}
