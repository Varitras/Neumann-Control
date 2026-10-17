//! Level meters: the shown level and a peak held for a while. Times are seconds of the UI clock.

pub const PEAK_HOLD_SECONDS: f64 = 1.5;
pub const METER_FLOOR_DB: f64 = -60.0;
pub const METER_CEILING_DB: f64 = 6.0;

/// Maps a dB value linearly from the floor..ceiling range to 0..1; the device reports silence
/// as -160, which lands below the floor and gives 0.
pub fn fraction(db: f64) -> f32 {
    let position = (db - METER_FLOOR_DB) / (METER_CEILING_DB - METER_FLOOR_DB);
    position.clamp(0.0, 1.0) as f32
}

#[derive(Clone, Copy, Debug)]
pub struct MeterState {
    level: f64,
    peak: f64,
    peak_since: f64,
}

impl Default for MeterState {
    fn default() -> MeterState {
        MeterState {
            level: METER_FLOOR_DB,
            peak: METER_FLOOR_DB,
            peak_since: 0.0,
        }
    }
}

impl MeterState {
    pub fn update(&mut self, level: f64, now: f64) {
        self.level = level;
        let hold_expired = now - self.peak_since >= PEAK_HOLD_SECONDS;
        if level >= self.peak || hold_expired {
            self.peak = level;
            self.peak_since = now;
        }
    }

    pub fn level(&self) -> f64 {
        self.level
    }

    /// The held peak, or the current level once the hold ran out.
    pub fn peak(&self, now: f64) -> f64 {
        if now - self.peak_since >= PEAK_HOLD_SECONDS {
            return self.level;
        }
        self.peak
    }

    pub fn reset(&mut self) {
        *self = MeterState::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_holds_then_falls_after_1_5_s() {
        let mut meter = MeterState::default();
        meter.update(-6.0, 0.0);
        meter.update(-30.0, 1.0);
        assert_eq!(meter.peak(1.0), -6.0);
        assert_eq!(meter.level(), -30.0);
        assert_eq!(meter.peak(1.25), -6.0);
        assert_eq!(meter.peak(1.5), -30.0);
    }

    #[test]
    fn meter_fraction_clamps_below_floor_and_above_ceiling() {
        assert_eq!(fraction(-160.0), 0.0);
        assert_eq!(fraction(10.0), 1.0);
        assert_eq!(fraction(-27.0), 0.5);
    }
}
