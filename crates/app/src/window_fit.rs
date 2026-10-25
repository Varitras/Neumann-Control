//! A window without a saved size for its mode (desk or compact): as large as the content needs,
//! growing with it (a speaker added, a strip that widens once its model is known) until the user
//! sizes the window. Sizes are points.

use eframe::egui::Vec2;

use crate::window_placement::WINDOW_MIN_SIZE;

/// A difference below this is rounding, not a size to ask for.
const TOLERANCE: f32 = 1.0;
/// Kept free around a window on the screen (points), for the taskbar.
pub const SCREEN_MARGIN: f32 = 60.0;

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
    /// `wanted` what the desk needs, `screen` the monitor's size when known, all in points of
    /// `pixels_per_point`.
    pub fn next(
        &mut self,
        current: Vec2,
        wanted: Vec2,
        screen: Option<Vec2>,
        pixels_per_point: f32,
    ) -> Option<Vec2> {
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
        let wanted = whole_pixels(bounded(wanted, screen), pixels_per_point);
        if close(current, wanted) {
            return None;
        }
        self.pending = Some((wanted, current));
        Some(wanted)
    }
}

/// The window size at which `content` just fills the visible part of its scroll area: the
/// window grown or shrunk by what the content lacks or leaves there. Everything else in the window
/// (toolbar, margins, scroll bars being shown) stays as it is; once the content fits, the bars go
/// and the next request takes their room back.
pub fn wanted_size(window: Vec2, visible: Vec2, content: Vec2) -> Vec2 {
    window + content - visible
}

fn bounded(wanted: Vec2, screen: Option<Vec2>) -> Vec2 {
    let at_least = wanted.max(Vec2::from(WINDOW_MIN_SIZE));
    match screen {
        Some(screen) => at_least.min(screen - Vec2::splat(SCREEN_MARGIN)),
        None => at_least,
    }
}

/// `size` grown to whole pixels: the window takes whole pixels, and a fraction short of the
/// content brings up scroll bars.
fn whole_pixels(size: Vec2, pixels_per_point: f32) -> Vec2 {
    (size * pixels_per_point).ceil() / pixels_per_point
}

fn close(one: Vec2, other: Vec2) -> bool {
    (one - other).abs().max_elem() < TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    /// Measured at 250 % on the first start: a scroll bar was showing, so the window was 22.9
    /// points wider than its visible area, not the 16 of the margins alone. Content plus margins
    /// asked for the size the window already had, and the bars stayed for good.
    #[test]
    fn the_wanted_size_counts_scroll_bars_being_shown() {
        let wanted = wanted_size(vec2(719.2, 654.8), vec2(696.3, 608.0), vec2(702.9, 609.6));
        assert!((wanted.x - 725.8).abs() < 1e-3, "{wanted:?}");
        assert!((wanted.y - 656.4).abs() < 1e-3, "{wanted:?}");
    }

    /// At 250 % the window takes whole pixels: 718.9 points became 1797 pixels, 718.8 points,
    /// a tenth too narrow; the desk then showed scroll bars, which kept it too small for good.
    #[test]
    fn the_request_is_rounded_up_to_whole_pixels() {
        let mut fit = WindowFit::default();
        let asked = fit.next(START, vec2(718.9, 654.6), SCREEN, 2.5).unwrap();
        assert!((asked.x - 719.2).abs() < 1e-3, "{asked:?}");
        assert!((asked.y - 654.8).abs() < 1e-3, "{asked:?}");
    }

    const START: Vec2 = Vec2::new(900.0, 600.0);
    const SCREEN: Option<Vec2> = Some(Vec2::new(1920.0, 1080.0));

    #[test]
    fn the_window_is_asked_for_what_the_desk_needs() {
        let mut fit = WindowFit::default();
        assert_eq!(
            fit.next(START, vec2(700.0, 800.0), SCREEN, 1.0),
            Some(vec2(700.0, 800.0))
        );
    }

    #[test]
    fn nothing_is_asked_while_a_request_is_under_way_or_when_it_fits() {
        let mut fit = WindowFit::default();
        let wanted = vec2(700.0, 800.0);
        fit.next(START, wanted, SCREEN, 1.0);
        assert_eq!(fit.next(START, wanted, SCREEN, 1.0), None, "under way");
        assert_eq!(fit.next(wanted, wanted, SCREEN, 1.0), None, "fits");
    }

    #[test]
    fn it_grows_with_the_desk() {
        let mut fit = WindowFit::default();
        fit.next(START, vec2(700.0, 800.0), SCREEN, 1.0);
        fit.next(vec2(700.0, 800.0), vec2(700.0, 800.0), SCREEN, 1.0);
        assert_eq!(
            fit.next(vec2(700.0, 800.0), vec2(900.0, 800.0), SCREEN, 1.0),
            Some(vec2(900.0, 800.0))
        );
    }

    #[test]
    fn a_window_the_user_sized_is_left_alone() {
        let mut fit = WindowFit::default();
        fit.next(START, vec2(700.0, 800.0), SCREEN, 1.0);
        fit.next(vec2(700.0, 800.0), vec2(700.0, 800.0), SCREEN, 1.0);
        fit.next(vec2(700.0, 800.0), vec2(900.0, 800.0), SCREEN, 1.0);
        assert_eq!(
            fit.next(vec2(1000.0, 500.0), vec2(900.0, 800.0), SCREEN, 1.0),
            None
        );
        assert_eq!(
            fit.next(vec2(1000.0, 500.0), vec2(1200.0, 900.0), SCREEN, 1.0),
            None
        );
    }

    #[test]
    fn the_request_stays_within_the_least_size_and_the_screen() {
        let mut fit = WindowFit::default();
        assert_eq!(
            fit.next(START, vec2(200.0, 3000.0), SCREEN, 1.0),
            Some(vec2(WINDOW_MIN_SIZE[0], 1080.0 - SCREEN_MARGIN))
        );
    }
}
