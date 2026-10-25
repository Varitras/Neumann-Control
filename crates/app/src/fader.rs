//! The level fader: its catalog range and wheel step.

use std::ops::RangeInclusive;

use neumann_control_core::catalog::Catalog;

use crate::paths;

pub const WHEEL_STEP_DB: f64 = 0.5;

/// The catalog range of the output level, which the fader drives.
pub fn output_level_range() -> RangeInclusive<f64> {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let entry =
        paths::catalog_entry(Catalog::embedded(), &path).expect("catalog has the output level");
    let min = entry.min.expect("output level has a minimum");
    let max = entry.max.expect("output level has a maximum");
    min..=max
}
