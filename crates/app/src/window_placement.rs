//! Where the window reopens: the saved geometry, moved onto a monitor that is still there
//! (an undocked laptop would otherwise open it off screen). All values are physical pixels.

use neumann_control_core::settings::WindowGeometry;

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
