//! How much goes into the log file: info lines always, with **Detailed log** also the debug
//! lines (every write with its value, every failed exchange, where windows open), for
//! troubleshooting.

use log::LevelFilter;

/// Switches the log between info and debug; the file logger itself takes everything up to debug.
pub fn apply(detailed: bool) {
    let level = if detailed {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    log::set_max_level(level);
}
