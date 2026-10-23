//! The first window without a saved size: as large as the desk needs, growing with it (a speaker
//! added, a strip that widens once its model is known) until the user sizes the window. Sizes
//! are points.

use eframe::egui::Vec2;

use crate::window_placement::WINDOW_MIN_SIZE;

/// A difference below this is rounding, not a size to ask for.
const TOLERANCE: f32 = 1.0;
/// Kept free around the window on the screen.
const SCREEN_MARGIN: f32 = 60.0;

#[derive(Debug, Default)]
pub struct WindowFit {
    /// The size asked for and the size the window had then; until the window has one of them
    /// the request is under way.
    pending: Option<(Vec2, Vec2)>,
    /// The user sized the window: the fit is over.
    done: bool,
}

impl WindowFit {
    /// The size to ask the window for this frame, if any. `current` is the window's inner size,
    /// `wanted` what the desk needs, `screen` the monitor's size when known.
    pub fn next(&mut self, current: Vec2, wanted: Vec2, screen: Option<Vec2>) -> Option<Vec2> {
        if self.done {
            return None;
        }
        if let Some((asked, before)) = self.pending {
            if close(current, before) {
                return None;
            }
            if !close(current, asked) {
                self.done = true;
                return None;
            }
            self.pending = None;
        }
        let wanted = bounded(wanted, screen);
        if close(current, wanted) {
            return None;
        }
        self.pending = Some((wanted, current));
        Some(wanted)
    }
}

fn bounded(wanted: Vec2, screen: Option<Vec2>) -> Vec2 {
    let at_least = wanted.max(Vec2::from(WINDOW_MIN_SIZE));
    match screen {
        Some(screen) => at_least.min(screen - Vec2::splat(SCREEN_MARGIN)),
        None => at_least,
    }
}

fn close(one: Vec2, other: Vec2) -> bool {
    (one - other).abs().max_elem() < TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    const START: Vec2 = Vec2::new(900.0, 600.0);
    const SCREEN: Option<Vec2> = Some(Vec2::new(1920.0, 1080.0));

    #[test]
    fn the_window_is_asked_for_what_the_desk_needs() {
        let mut fit = WindowFit::default();
        assert_eq!(
            fit.next(START, vec2(700.0, 800.0), SCREEN),
            Some(vec2(700.0, 800.0))
        );
    }

    #[test]
    fn nothing_is_asked_while_a_request_is_under_way_or_when_it_fits() {
        let mut fit = WindowFit::default();
        let wanted = vec2(700.0, 800.0);
        fit.next(START, wanted, SCREEN);
        assert_eq!(fit.next(START, wanted, SCREEN), None, "under way");
        assert_eq!(fit.next(wanted, wanted, SCREEN), None, "fits");
    }

    #[test]
    fn it_grows_with_the_desk() {
        let mut fit = WindowFit::default();
        fit.next(START, vec2(700.0, 800.0), SCREEN);
        fit.next(vec2(700.0, 800.0), vec2(700.0, 800.0), SCREEN);
        assert_eq!(
            fit.next(vec2(700.0, 800.0), vec2(900.0, 800.0), SCREEN),
            Some(vec2(900.0, 800.0))
        );
    }

    #[test]
    fn a_window_the_user_sized_is_left_alone() {
        let mut fit = WindowFit::default();
        fit.next(START, vec2(700.0, 800.0), SCREEN);
        fit.next(vec2(700.0, 800.0), vec2(700.0, 800.0), SCREEN);
        fit.next(vec2(700.0, 800.0), vec2(900.0, 800.0), SCREEN);
        assert_eq!(
            fit.next(vec2(1000.0, 500.0), vec2(900.0, 800.0), SCREEN),
            None
        );
        assert_eq!(
            fit.next(vec2(1000.0, 500.0), vec2(1200.0, 900.0), SCREEN),
            None
        );
    }

    #[test]
    fn the_request_stays_within_the_least_size_and_the_screen() {
        let mut fit = WindowFit::default();
        assert_eq!(
            fit.next(START, vec2(200.0, 3000.0), SCREEN),
            Some(vec2(WINDOW_MIN_SIZE[0], 1080.0 - SCREEN_MARGIN))
        );
    }
}
