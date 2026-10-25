//! How much goes into the log file: info lines always, with **Detailed log** also the debug
//! lines (every write with its value, every failed exchange, where windows open, the frame
//! rate), for troubleshooting.

use log::LevelFilter;

/// Switches the log between info and debug; the file logger itself takes everything up to debug.
pub fn apply(detailed: bool) {
    log::set_max_level(level(detailed));
}

fn level(detailed: bool) -> LevelFilter {
    if detailed {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_detailed_log_takes_debug_lines_and_the_normal_one_info() {
        assert_eq!(level(true), LevelFilter::Debug);
        assert_eq!(level(false), LevelFilter::Info);
    }
}
