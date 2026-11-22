//! A window without a saved size for its mode (desk or compact): as large as the content needs,
//! growing with it (a speaker added, a strip that widens once its model is known) until the user
//! sizes the window. Sizes are points.

use eframe::egui::Vec2;

use crate::window_placement::min_size;

/// A difference below this is rounding, not a size to ask for, as long as the window is not
/// short of the content: at 1.25 px/pt a whole pixel is less than this.
const TOLERANCE: f32 = 1.0;
/// Less than this apart, two sizes are the same whole pixels.
const HALF_PIXEL: f32 = 0.5;
/// Points times scale land this far above a whole pixel from float noise alone (egui gives the
/// window's pixels divided by its scale); it is no pixel more.
const PIXEL_NOISE: f32 = 0.001;
/// Below this a window short of the content is float noise, not a pixel.
const SHORT_BY: f32 = 0.01;
/// Kept free around a window on the screen (points), for the taskbar.
pub const SCREEN_MARGIN: f32 = 60.0;

#[derive(Debug)]
pub struct WindowFit {
    /// The mode's smallest window: the fit asks for no less.
    least: Vec2,
    /// The size asked for and the size the window had then; until the window has one of them
    /// the request is under way.
    pending: Option<(Vec2, Vec2)>,
    /// The size the window had when there was nothing to ask; another one it was not asked for
    /// was given by the user (or the system).
    settled: Option<Vec2>,
    /// A size asked for and granted a little smaller: not asked for again.
    granted_short: Option<Vec2>,
    /// The user sized the window: the fit is over.
    done: bool,
}

impl Default for WindowFit {
    /// The compact mode's fit, whose least size is the smaller one.
    fn default() -> WindowFit {
        WindowFit::for_mode(true)
    }
}

impl WindowFit {
    /// A fit for the desk or the compact mode, never below its least size.
    pub fn for_mode(compact: bool) -> WindowFit {
        WindowFit {
            least: Vec2::from(min_size(compact)),
            pending: None,
            settled: None,
            granted_short: None,
            done: false,
        }
    }

    /// The user sized the window (or the system gave it another size): from now on its size is
    /// a choice.
    pub fn is_over(&self) -> bool {
        self.done
    }

    /// The size to ask the window for this frame, if any. `current` is the window's inner size,
    /// `wanted` what the content of the current mode (desk or compact) needs, `screen` the
    /// monitor's size when known, all in points of `pixels_per_point`.
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
            // Granted to the pixel first: a request less than a point more is also close to the
            // size before, and waited for good.
            let granted = same_pixels(current, asked, pixels_per_point);
            if !granted && close(current, before) {
                return None;
            }
            if !granted && !close(current, asked) {
                log::debug!("window fit: over, asked {asked:?} and got {current:?}");
                self.done = true;
                return None;
            }
            self.pending = None;
            self.granted_short = short_of(current, asked).then_some(asked);
        } else if let Some(settled) = self.settled {
            if !close(current, settled) {
                log::debug!("window fit: over, the window went from {settled:?} to {current:?}");
                self.done = true;
                return None;
            }
        }
        let wanted = whole_pixels(bounded(wanted, self.least, screen), pixels_per_point);
        let fits = close(current, wanted) && !short_of(current, wanted);
        // The system may grant a pixel less than asked; asking again would only repeat.
        if fits || self.granted_short == Some(wanted) {
            if self.settled.is_none() {
                log::debug!("window fit: {current:?} fits");
            }
            self.settled = Some(current);
            return None;
        }
        self.settled = None;
        self.pending = Some((wanted, current));
        Some(wanted)
    }
}

/// The window size at which `content` just fills `room`, the whole room its scroll area gets
/// (scroll bars included): the window grown or shrunk by what the content lacks or leaves there,
/// everything else in it (toolbar, margins) as it is. Not the part the scroll area shows: that
/// loses a bar's room while one shows or fades in, so each size asked for another.
pub fn wanted_size(window: Vec2, room: Vec2, content: Vec2) -> Vec2 {
    window + content - room
}

fn bounded(wanted: Vec2, least: Vec2, screen: Option<Vec2>) -> Vec2 {
    let at_least = wanted.max(least);
    match screen {
        Some(screen) => at_least.min(screen - Vec2::splat(SCREEN_MARGIN)),
        None => at_least,
    }
}

/// `size` grown to whole pixels: the window takes whole pixels, and a fraction short of the
/// content brings up scroll bars.
fn whole_pixels(size: Vec2, pixels_per_point: f32) -> Vec2 {
    let pixels = size * pixels_per_point - Vec2::splat(PIXEL_NOISE);
    pixels.ceil() / pixels_per_point
}

/// `current` smaller than `wanted` in either direction, by more than float noise.
fn short_of(current: Vec2, wanted: Vec2) -> bool {
    current.x < wanted.x - SHORT_BY || current.y < wanted.y - SHORT_BY
}

fn close(one: Vec2, other: Vec2) -> bool {
    (one - other).abs().max_elem() < TOLERANCE
}

/// The same size in the window's whole pixels.
fn same_pixels(one: Vec2, other: Vec2, pixels_per_point: f32) -> bool {
    ((one - other) * pixels_per_point).abs().max_elem() < HALF_PIXEL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_placement::COMPACT_MIN_SIZE;
    use eframe::egui::vec2;

    /// A window of whole pixels, as egui gives it in points (pixels / scale, with float noise),
    /// already fits a content of its own size: the noise above a whole pixel asked for one more.
    #[test]
    fn a_window_of_whole_pixels_fits_its_own_size() {
        for pixels_per_point in [1.25_f32, 1.5, 1.75] {
            for pixels in 1000..1800 {
                let current = Vec2::splat(pixels as f32 / pixels_per_point);
                let mut fit = WindowFit::default();
                let asked = fit.next(current, current, None, pixels_per_point);
                assert_eq!(asked, None, "{pixels} px at {pixels_per_point}");
            }
        }
    }

    /// Measured on Linux: the window swung between 720×658 and 726×655 for good. Measured by
    /// the room of the scroll area (16 and 45 points less than the window), both sizes want the
    /// same window, whatever the scroll bars did there.
    #[test]
    fn both_sizes_the_window_swung_between_want_the_same() {
        let content = vec2(702.9, 610.0);
        let narrow = wanted_size(vec2(720.0, 658.0), vec2(704.0, 613.0), content);
        let wide = wanted_size(vec2(726.0, 655.0), vec2(710.0, 610.0), content);
        assert!((narrow - wide).length() < 1e-3, "{narrow:?} {wide:?}");
        assert!((narrow - vec2(718.9, 655.0)).length() < 1e-3, "{narrow:?}");
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

    /// Above 100 % a request can be less than a point more than the window has. Granted, the new
    /// size was still "close to" the old one, so the request counted as under way for good and
    /// the window never grew with the desk again.
    #[test]
    fn a_request_less_than_a_point_more_is_seen_granted() {
        const SCALE: f32 = 1.25;
        let mut fit = WindowFit::default();
        let current = vec2(640.0, 480.0);
        let asked = fit
            .next(current, vec2(640.3, 480.0), SCREEN, SCALE)
            .expect("a request");
        assert!((asked - current).max_elem() < TOLERANCE, "{asked:?}");
        assert_eq!(fit.next(asked, vec2(640.3, 480.0), SCREEN, SCALE), None);
        let grown = fit.next(asked, vec2(740.0, 480.0), SCREEN, SCALE);
        assert!(grown.is_some(), "the fit hangs on its earlier request");
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

    /// Under Wayland the app's own request won over the user's resize, so the fit never saw a
    /// size it had not asked for while one was under way, and set the user's size back. A size
    /// the window takes without being asked ends the fit just as well.
    #[test]
    fn a_window_sized_after_the_fit_settled_is_left_alone() {
        let mut fit = WindowFit::default();
        let wanted = vec2(700.0, 800.0);
        fit.next(START, wanted, SCREEN, 1.0);
        assert_eq!(fit.next(wanted, wanted, SCREEN, 1.0), None, "fits");
        let sized = vec2(844.0, 800.0);
        assert_eq!(fit.next(sized, wanted, SCREEN, 1.0), None);
        assert_eq!(fit.next(sized, vec2(900.0, 800.0), SCREEN, 1.0), None);
    }

    /// At 1.25 px/pt a pixel is 0.8 points, less than the rounding allowed: content that grew by
    /// part of a pixel after the window fitted counted as fitting, and the window stayed a pixel
    /// short with scroll bars (Linux VM). Short by any pixel asks again.
    #[test]
    fn a_window_a_pixel_short_of_the_content_is_asked_to_grow() {
        let mut fit = WindowFit::default();
        let fitted = vec2(719.2, 654.4);
        assert_eq!(fit.next(fitted, fitted, SCREEN, 1.25), None, "fits");
        assert_eq!(
            fit.next(fitted, vec2(719.5, 654.4), SCREEN, 1.25),
            Some(vec2(720.0, 654.4))
        );
    }

    /// A system that grants a pixel less than asked is not asked again and again.
    #[test]
    fn a_pixel_less_than_asked_is_taken_as_granted() {
        let mut fit = WindowFit::default();
        let wanted = vec2(720.0, 654.4);
        fit.next(START, wanted, SCREEN, 1.25);
        assert_eq!(fit.next(vec2(719.2, 654.4), wanted, SCREEN, 1.25), None);
        assert_eq!(fit.next(vec2(719.2, 654.4), wanted, SCREEN, 1.25), None);
    }

    #[test]
    fn the_request_stays_within_the_least_size_and_the_screen() {
        let mut fit = WindowFit::default();
        assert_eq!(
            fit.next(START, vec2(200.0, 3000.0), SCREEN, 1.0),
            Some(vec2(COMPACT_MIN_SIZE[0], 1080.0 - SCREEN_MARGIN))
        );
    }
}
