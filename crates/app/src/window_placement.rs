//! Where the window reopens: the saved geometry, moved onto a monitor that is still there
//! (an undocked laptop would otherwise open it off screen). At the start and when the window
//! switches between desk and compact mode. All geometry values are physical pixels.

use neumann_control_core::settings::WindowGeometry;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::Window;

/// The smallest window in points, in both modes.
pub const WINDOW_MIN_SIZE: [f32; 2] = [480.0, 360.0];
/// The window size in points of a mode that never had a window of its own.
pub const DESK_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
pub const COMPACT_WINDOW_SIZE: [f32; 2] = [640.0, 360.0];

pub fn first_window_size(compact: bool) -> [f32; 2] {
    if compact {
        COMPACT_WINDOW_SIZE
    } else {
        DESK_WINDOW_SIZE
    }
}

/// The monitors there are now, in physical pixels.
pub fn monitor_areas(window: &Window) -> Vec<MonitorArea> {
    window
        .available_monitors()
        .map(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            MonitorArea {
                x: position.x as f32,
                y: position.y as f32,
                width: size.width as f32,
                height: size.height as f32,
            }
        })
        .collect()
}

/// Puts the window where `saved` says, fitted onto the monitors there are now. Set in physical
/// pixels once the window exists: points need the scale, which is known only on a monitor.
pub fn place_window(window: &Window, saved: WindowGeometry) {
    let scale = window.scale_factor() as f32;
    let min_size = (WINDOW_MIN_SIZE[0] * scale, WINDOW_MIN_SIZE[1] * scale);
    let geometry = fit_into(saved, &monitor_areas(window), min_size);
    log::debug!("placing the window: saved {saved:?}, asked {geometry:?} at scale {scale}");
    window.set_outer_position(PhysicalPosition::new(geometry.x, geometry.y));
    // The size granted arrives as a resize event like any other.
    let _ = window.request_inner_size(PhysicalSize::new(geometry.width, geometry.height));
    // A new window is not maximized: under Wayland "not maximized" told anyway brought the
    // compositor's size back over the one just asked for.
    if geometry.maximized {
        window.set_maximized(true);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The saved geometry on the monitor holding its centre, else on the first monitor; shrunk to
/// that monitor but never below `min_size`. Without monitors (none could be listed) the saved
/// geometry is used as it is.
pub fn fit_into(
    saved: WindowGeometry,
    monitors: &[MonitorArea],
    min_size: (f32, f32),
) -> WindowGeometry {
    let Some(first) = monitors.first() else {
        return saved;
    };
    let centre_x = saved.x + saved.width / 2.0;
    let centre_y = saved.y + saved.height / 2.0;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.contains(centre_x, centre_y))
        .unwrap_or(first);
    let width = saved.width.min(monitor.width).max(min_size.0);
    let height = saved.height.min(monitor.height).max(min_size.1);
    WindowGeometry {
        x: clamp_start(saved.x, monitor.x, monitor.width, width),
        y: clamp_start(saved.y, monitor.y, monitor.height, height),
        width,
        height,
        maximized: saved.maximized,
    }
}

impl MonitorArea {
    fn contains(&self, x: f32, y: f32) -> bool {
        let inside_x = x >= self.x && x < self.x + self.width;
        let inside_y = y >= self.y && y < self.y + self.height;
        inside_x && inside_y
    }
}

/// Keeps the window inside the monitor along one axis; a window longer than the monitor
/// (the minimum size wins) starts at the monitor's edge.
fn clamp_start(start: f32, monitor_start: f32, monitor_length: f32, window_length: f32) -> f32 {
    let last_start = monitor_start + monitor_length - window_length;
    if last_start < monitor_start {
        return monitor_start;
    }
    start.clamp(monitor_start, last_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_SIZE: (f32, f32) = (480.0, 360.0);
    const LAPTOP: MonitorArea = MonitorArea {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    const DOCKED: MonitorArea = MonitorArea {
        x: 1920.0,
        y: 0.0,
        width: 2560.0,
        height: 1440.0,
    };

    fn geometry(x: f32, y: f32, width: f32, height: f32) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn a_window_on_a_present_monitor_stays_where_it_was() {
        let saved = geometry(2100.0, 100.0, 900.0, 600.0);
        assert_eq!(fit_into(saved, &[LAPTOP, DOCKED], MIN_SIZE), saved);
    }

    #[test]
    fn a_window_on_a_monitor_that_is_gone_moves_onto_the_first_one() {
        let saved = geometry(2100.0, 100.0, 900.0, 600.0);
        assert_eq!(
            fit_into(saved, &[LAPTOP], MIN_SIZE),
            geometry(1020.0, 100.0, 900.0, 600.0)
        );
    }

    #[test]
    fn a_window_hanging_over_the_edge_is_pulled_in() {
        let saved = geometry(-300.0, 900.0, 900.0, 600.0);
        assert_eq!(
            fit_into(saved, &[LAPTOP], MIN_SIZE),
            geometry(0.0, 480.0, 900.0, 600.0)
        );
    }

    #[test]
    fn a_window_larger_than_its_monitor_shrinks_but_not_below_the_minimum() {
        let small = MonitorArea {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 800.0,
        };
        let saved = geometry(0.0, 0.0, 3000.0, 2000.0);
        assert_eq!(
            fit_into(saved, &[small], MIN_SIZE),
            geometry(0.0, 0.0, 480.0, 800.0)
        );
    }

    #[test]
    fn without_monitors_the_saved_geometry_is_kept() {
        let saved = geometry(-5000.0, -5000.0, 900.0, 600.0);
        assert_eq!(fit_into(saved, &[], MIN_SIZE), saved);
    }
}
