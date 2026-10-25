//! The windows of their own (device panel, EQ, all values, settings, presets) open where the
//! user last left one of their kind, for every speaker alike. Kept in the settings by kind, in
//! screen pixels like the main window.

use std::collections::BTreeMap;

use eframe::egui::{pos2, vec2, Context, Vec2, ViewportId};
use neumann_control_core::settings::WindowGeometry;

use super::{window_geometry, NeumannApp};
use crate::views::{Opening, WindowPlace};
use crate::window_placement::{fit_into, WINDOW_MIN_SIZE};

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
    /// while open, else where the last of its kind was left (or at its default).
    pub(super) fn opening(
        &self,
        context: &Context,
        viewport: ViewportId,
        kind: WindowKind,
        index: usize,
    ) -> Opening {
        let open = context.input_for(viewport, |input| input.viewport().outer_rect.is_some());
        if open {
            return Opening::Open;
        }
        let saved = self
            .saved_place(context, kind)
            .map_or(Opening::Default, Opening::At);
        let opening = cascaded(saved, index);
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
        let pixels_per_point = context.pixels_per_point();
        let least = (
            WINDOW_MIN_SIZE[0] * pixels_per_point,
            WINDOW_MIN_SIZE[1] * pixels_per_point,
        );
        // On a monitor there is now: the saved one may have been unplugged since.
        let fitted = fit_into(*saved, &self.monitors, least);
        Some(place_of(fitted, pixels_per_point))
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
            window_geometry(input.viewport(), input.pixels_per_point, last)
        });
        if let Some(geometry) = geometry {
            keep_place(&mut self.settings.windows, kind, geometry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
