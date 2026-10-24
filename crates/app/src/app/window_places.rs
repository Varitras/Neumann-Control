//! The windows of their own (device panel, EQ, all values) open where the user last left them.
//! Kept in the settings by kind and speaker, in screen pixels like the main window.

use eframe::egui::{pos2, vec2, Context, ViewportId};
use neumann_control_core::settings::WindowGeometry;

use super::{window_geometry, NeumannApp};
use crate::groups::member_id;
use crate::state::SpeakerKey;
use crate::views::{Opening, WindowPlace};
use crate::window_placement::{fit_into, WINDOW_MIN_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowKind {
    Panel,
    Eq,
    AllValues,
}

impl WindowKind {
    pub(super) const ALL: [WindowKind; 3] =
        [WindowKind::Panel, WindowKind::Eq, WindowKind::AllValues];

    fn name(self) -> &'static str {
        match self {
            WindowKind::Panel => "panel",
            WindowKind::Eq => "eq",
            WindowKind::AllValues => "all_values",
        }
    }
}

/// Where the `kind` window of the speaker with identity `member` is kept in the settings.
pub(super) fn place_key(kind: WindowKind, member: &str) -> String {
    format!("{} {}", kind.name(), member)
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
    /// How the `kind` window of `key` on `viewport` is drawn: left alone while open, else where
    /// it was left (or at its default).
    pub(super) fn opening(
        &self,
        context: &Context,
        viewport: ViewportId,
        kind: WindowKind,
        key: &SpeakerKey,
    ) -> Opening {
        let open = context.input_for(viewport, |input| input.viewport().outer_rect.is_some());
        if open {
            return Opening::Open;
        }
        self.saved_place(context, kind, key)
            .map_or(Opening::Default, Opening::At)
    }

    /// Where the `kind` window of `key` was left, if it was.
    fn saved_place(
        &self,
        context: &Context,
        kind: WindowKind,
        key: &SpeakerKey,
    ) -> Option<WindowPlace> {
        let saved = self
            .settings
            .windows
            .get(&place_key(kind, &member_id(key)))?;
        let pixels_per_point = context.pixels_per_point();
        let least = (
            WINDOW_MIN_SIZE[0] * pixels_per_point,
            WINDOW_MIN_SIZE[1] * pixels_per_point,
        );
        // On a monitor there is now: the saved one may have been unplugged since.
        let fitted = fit_into(*saved, &self.monitors, least);
        Some(place_of(fitted, pixels_per_point))
    }

    /// Notes where the window on `viewport` is now; a minimised, maximised or closed one keeps
    /// its last place.
    pub(super) fn remember_place(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        kind: WindowKind,
        key: &SpeakerKey,
    ) {
        // The window's own scale: on another monitor it may differ from the main window's.
        let place_key = place_key(kind, &member_id(key));
        let last = self.settings.windows.get(&place_key).copied();
        let geometry = context.input_for(viewport, |input| {
            window_geometry(input.viewport(), input.pixels_per_point, last)
        });
        if let Some(geometry) = geometry {
            self.settings.windows.insert(place_key, geometry);
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

    #[test]
    fn each_kind_and_speaker_has_its_own_place() {
        let member = member_id(&SpeakerKey::Serial("SIM0001234".to_owned()));
        assert_eq!(place_key(WindowKind::Eq, &member), "eq serial:SIM0001234");
        assert_ne!(
            place_key(WindowKind::Panel, &member),
            place_key(WindowKind::AllValues, &member)
        );
    }
}
