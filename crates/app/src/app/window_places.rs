//! The windows of their own (device panel, EQ, all values, settings, presets) open where the
//! user last left one of their kind, for every speaker alike. Kept in the settings by kind, in
//! screen pixels like the main window.

use std::collections::BTreeMap;

use eframe::egui::{pos2, vec2, Context, Pos2, Rect, Vec2, ViewportCommand, ViewportId};
use neumann_control_core::settings::WindowGeometry;

use super::{is_open, window_geometry, AwaitedWindow, NeumannApp};
use crate::views::{
    all_values, eq_window, groups_window, panel, preset_dialog, settings_dialog, Opening,
    WindowPlace,
};
use crate::window_fit::SCREEN_MARGIN;
use crate::window_placement::{fit_into, MonitorArea, WINDOW_MIN_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowKind {
    Panel,
    Eq,
    AllValues,
    Settings,
    Preset,
    Groups,
}

/// How far each further open window of a kind is set off from the one before, so they do not
/// cover each other exactly.
const CASCADE_STEP: Vec2 = Vec2::new(30.0, 30.0);

impl WindowKind {
    #[cfg(test)]
    const ALL: [WindowKind; 6] = [
        WindowKind::Panel,
        WindowKind::Eq,
        WindowKind::AllValues,
        WindowKind::Settings,
        WindowKind::Preset,
        WindowKind::Groups,
    ];

    /// The least size its builder gives the window, in points.
    fn least_size(self) -> [f32; 2] {
        match self {
            WindowKind::Panel => panel::PANEL_WINDOW_MIN_SIZE,
            WindowKind::Eq => eq_window::EQ_WINDOW_MIN_SIZE,
            WindowKind::AllValues => all_values::ALL_VALUES_MIN_SIZE,
            WindowKind::Settings => settings_dialog::SETTINGS_WINDOW_MIN_SIZE,
            WindowKind::Preset => preset_dialog::WINDOW_MIN_SIZE,
            WindowKind::Groups => groups_window::GROUPS_WINDOW_MIN_SIZE,
        }
    }

    /// Where the kind's window is kept in the settings.
    fn key(self) -> &'static str {
        match self {
            WindowKind::Panel => "panel",
            WindowKind::Eq => "eq",
            WindowKind::AllValues => "all_values",
            WindowKind::Settings => "settings",
            WindowKind::Preset => "preset",
            WindowKind::Groups => "groups",
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

/// The `index`-th open window of a kind (from 0), set off from the others; before it is fitted
/// onto a monitor, so that the offset never takes it past the screen's edge.
fn cascaded(geometry: WindowGeometry, index: usize, pixels_per_point: f32) -> WindowGeometry {
    let offset = CASCADE_STEP * index as f32 * pixels_per_point;
    WindowGeometry {
        x: geometry.x + offset.x,
        y: geometry.y + offset.y,
        ..geometry
    }
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

/// A window of its own at `geometry` on a monitor there is, no smaller than its kind's own
/// least size (the main window's is larger than that of settings and presets).
fn own_window_place(
    geometry: WindowGeometry,
    monitors: &[MonitorArea],
    pixels_per_point: f32,
    kind: WindowKind,
) -> WindowPlace {
    let [width, height] = kind.least_size();
    let least = (width * pixels_per_point, height * pixels_per_point);
    place_of(fit_into(geometry, monitors, least), pixels_per_point)
}

/// The `index`-th open window of `kind` (from 0) where the last of its kind was left (`saved`,
/// screen pixels).
fn saved_window(
    saved: WindowGeometry,
    index: usize,
    monitors: &[MonitorArea],
    pixels_per_point: f32,
    kind: WindowKind,
) -> WindowPlace {
    let shifted = cascaded(saved, index, pixels_per_point);
    own_window_place(shifted, monitors, pixels_per_point, kind)
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

/// Windows creates a window at the place given in points with the scale of the monitor it
/// starts on; X11 and Wayland windows are left where the window manager puts them.
const MOVES_NEW_WINDOWS_BACK: bool = cfg!(windows);

/// Measured and asked-for places differ by up to this from rounding to whole pixels.
const ONE_PIXEL: f32 = 1.0;

/// Where a window opened at `intended` has to go when it is at `measured` (both in screen
/// pixels), in its own scale `pixels_per_point`; `None` where it is.
pub(super) fn misplaced(
    intended: WindowGeometry,
    measured: WindowGeometry,
    pixels_per_point: f32,
) -> Option<WindowPlace> {
    let off = |one: f32, other: f32| (one - other).abs() > ONE_PIXEL;
    let moved = off(intended.x, measured.x)
        || off(intended.y, measured.y)
        || off(intended.width, measured.width)
        || off(intended.height, measured.height);
    moved.then(|| place_of(intended, pixels_per_point))
}

/// A place (points, at `pixels_per_point`) in screen pixels.
fn pixels_of(place: WindowPlace, position: Pos2, pixels_per_point: f32) -> WindowGeometry {
    WindowGeometry {
        x: position.x * pixels_per_point,
        y: position.y * pixels_per_point,
        width: place.size.x * pixels_per_point,
        height: place.size.y * pixels_per_point,
        maximized: place.maximized,
    }
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
        &mut self,
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
            .saved_place(context, kind, index)
            .or_else(|| self.centred_place(context, default_size, index))
            .or_else(|| unplaced(default_size, monitor));
        let opening = place.map_or(Opening::Default, Opening::At);
        if let Opening::At(place) = opening {
            self.note_placing(context, viewport, place);
        }
        log::debug!(
            "{} window {index} opens {opening:?} on monitors {:?}",
            kind.key(),
            self.monitors
        );
        opening
    }

    /// A window opened at `place` (in the main window's scale) is checked once it is open.
    fn note_placing(&mut self, context: &Context, viewport: ViewportId, place: WindowPlace) {
        let Some(position) = place.position.filter(|_| MOVES_NEW_WINDOWS_BACK) else {
            return;
        };
        if place.maximized {
            return;
        }
        let pixels = pixels_of(place, position, context.pixels_per_point());
        self.placing.insert(viewport, pixels);
    }

    /// Where the last `kind` window was left, if one was, for the `index`-th open one.
    fn saved_place(
        &self,
        context: &Context,
        kind: WindowKind,
        index: usize,
    ) -> Option<WindowPlace> {
        let saved = self.settings.windows.get(kind.key())?;
        // On a monitor there is now: the saved one may have been unplugged since.
        let pixels_per_point = context.pixels_per_point();
        Some(saved_window(
            *saved,
            index,
            &self.monitors,
            pixels_per_point,
            kind,
        ))
    }

    /// The `index`-th window of `size` over the middle of the main window, once the main
    /// window's place is known.
    fn centred_place(&self, context: &Context, size: Vec2, index: usize) -> Option<WindowPlace> {
        let main = context.input_for(ViewportId::ROOT, |input| input.viewport().outer_rect)?;
        let pixels_per_point = context.pixels_per_point();
        let geometry = cascaded(
            centred(main, size, pixels_per_point),
            index,
            pixels_per_point,
        );
        let placed = above_the_taskbar(geometry, &self.monitors, pixels_per_point);
        Some(place_of(placed, pixels_per_point))
    }

    /// The main window to `geometry`, through egui as every size the fit asks for: set on the
    /// winit window directly, beside egui's commands, it came late or not at all under Wayland.
    pub(super) fn restore_window(&mut self, context: &Context, geometry: WindowGeometry) {
        let place = self.on_a_monitor(context, geometry);
        if let Some(position) = place.position {
            context.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        }
        context.send_viewport_cmd(ViewportCommand::InnerSize(place.size));
        // Only a change: under Wayland "not maximized" to a window that is not maximized brought
        // the compositor's old size back over the one just asked for.
        let maximized = context.input(|input| input.viewport().maximized == Some(true));
        if place.maximized != maximized {
            context.send_viewport_cmd(ViewportCommand::Maximized(place.maximized));
            let now = context.input(|input| input.time);
            self.awaited_window = Some(AwaitedWindow::new(geometry, now));
        }
    }

    /// The main window's `geometry` (screen pixels) moved and shrunk onto a monitor there is,
    /// as egui places it, never below the main window's least size.
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
        let (geometry, pixels_per_point) = context.input_for(viewport, |input| {
            let size = input.viewport_rect().size();
            let geometry = window_geometry(input.viewport(), size, input.pixels_per_point, last);
            (geometry, input.pixels_per_point)
        });
        let Some(geometry) = geometry else {
            return;
        };
        let intended = self.placing.remove(&viewport);
        if let Some(place) =
            intended.and_then(|intended| misplaced(intended, geometry, pixels_per_point))
        {
            // Its place this frame is the one Windows chose, not the user's.
            if let Some(position) = place.position {
                context.send_viewport_cmd_to(viewport, ViewportCommand::OuterPosition(position));
            }
            context.send_viewport_cmd_to(viewport, ViewportCommand::InnerSize(place.size));
            return;
        }
        keep_place(&mut self.settings.windows, kind, geometry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::ViewportInfo;

    /// Windows converts a new window's place with the scale of the monitor it starts on: with
    /// the main window at 100 % and that monitor at 150 %, a saved x of 3000 pixels became 4500.
    /// The window is moved back in its own scale once it is open.
    #[test]
    fn a_window_opened_at_another_scale_is_moved_to_its_place() {
        let intended = WindowGeometry {
            x: 3000.0,
            y: 200.0,
            width: 600.0,
            height: 400.0,
            maximized: false,
        };
        let measured = WindowGeometry {
            x: 4500.0,
            y: 300.0,
            width: 900.0,
            height: 600.0,
            ..intended
        };
        let place = misplaced(intended, measured, 1.5).expect("moved");
        assert_eq!(place.position, Some(pos2(2000.0, 133.33333)));
        assert_eq!(place.size, vec2(400.0, 266.66666));
        assert_eq!(misplaced(intended, intended, 1.0), None);
        let rounded = WindowGeometry {
            x: 3000.4,
            ..intended
        };
        assert_eq!(misplaced(rounded, intended, 1.0), None);
    }

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

    /// A window of its own made smaller than the main window may be (settings, presets) opens
    /// as small again: its own builder keeps its least size, not the main window's.
    #[test]
    fn a_window_of_its_own_keeps_a_size_below_the_main_windows_least() {
        let screen = MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let saved = WindowGeometry {
            x: 100.0,
            y: 100.0,
            width: 360.0,
            height: 260.0,
            maximized: false,
        };
        let place = own_window_place(saved, &[screen], 1.0, WindowKind::Settings);
        assert_eq!(place.size, vec2(360.0, 300.0));
    }

    /// A broken saved size (hand-edited, or kept while minimized without the system saying so)
    /// opens no smaller than the kind's own least size, not as a bare frame.
    #[test]
    fn a_window_of_its_own_opens_no_smaller_than_its_kind_allows() {
        let screen = MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let saved = WindowGeometry {
            x: 100.0,
            y: 100.0,
            width: 0.0,
            height: 0.0,
            maximized: false,
        };
        for kind in WindowKind::ALL {
            let place = own_window_place(saved, &[screen], 1.0, kind);
            assert_eq!(place.size, Vec2::from(kind.least_size()), "{kind:?}");
            assert!(place.size.x > 0.0 && place.size.y > 0.0, "{kind:?}");
        }
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
        let screen = [MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }];
        let kind = WindowKind::Panel;
        let first = saved_window(GEOMETRY, 0, &screen, 1.0, kind);
        assert_eq!(first, place_of(GEOMETRY, 1.0));
        let third = saved_window(GEOMETRY, 2, &screen, 1.0, kind);
        assert_eq!(third.position, Some(pos2(10.0, 20.0) + CASCADE_STEP * 2.0));
        assert_eq!(third.size, first.size);
    }

    /// The offset came after the fit onto the monitor: a window of a kind left at the screen's
    /// edge opened its further ones partly past it.
    #[test]
    fn a_further_window_at_the_screen_edge_stays_on_the_screen() {
        let screen = [MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }];
        let corner = WindowGeometry {
            x: 1320.0,
            y: 680.0,
            ..GEOMETRY
        };
        let third = saved_window(corner, 2, &screen, 1.0, WindowKind::Panel);
        let end = third.position.unwrap() + third.size;
        assert!(end.x <= 1920.0 && end.y <= 1080.0, "{third:?}");
    }
}
