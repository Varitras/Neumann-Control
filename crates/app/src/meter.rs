//! Level meters: the shown level and a peak held for a while. Times are seconds of the UI clock.

pub const PEAK_HOLD_SECONDS: f64 = 1.5;
/// Low enough that quiet room volume (inputs around -90 dB on the KH 120 II) still moves.
pub const METER_FLOOR_DB: f64 = -120.0;
pub const METER_CEILING_DB: f64 = 6.0;

/// Maps a dB value linearly from the floor..ceiling range to 0..1; the device reports silence
/// as -160, which lands below the floor and gives 0.
pub fn fraction(db: f64) -> f32 {
    let position = (db - METER_FLOOR_DB) / (METER_CEILING_DB - METER_FLOOR_DB);
    position.clamp(0.0, 1.0) as f32
}

/// The device is polled about ten times a second; the bar glides between the readings like a
/// peak meter instead of jumping: quick rise, steady fall.
pub const ATTACK_SECONDS: f64 = 0.08;
pub const RELEASE_DB_PER_SECOND: f64 = 24.0;

#[derive(Clone, Copy, Debug)]
pub struct MeterState {
    level: f64,
    /// Where the bar stood when `level` arrived, and when that was.
    start: f64,
    start_time: f64,
    peak: f64,
    peak_since: f64,
}

impl Default for MeterState {
    fn default() -> MeterState {
        MeterState {
            level: METER_FLOOR_DB,
            start: METER_FLOOR_DB,
            start_time: 0.0,
            peak: METER_FLOOR_DB,
            peak_since: 0.0,
        }
    }
}

impl MeterState {
    pub fn update(&mut self, level: f64, now: f64) {
        self.start = self.shown(now);
        self.start_time = now;
        self.level = level;
        let hold_expired = now - self.peak_since >= PEAK_HOLD_SECONDS;
        if level >= self.peak || hold_expired {
            self.peak = level;
            self.peak_since = now;
        }
    }

    /// The last reading of the device.
    pub fn level(&self) -> f64 {
        self.level
    }

    /// Where the bar stands at `now` on its way from the previous position to `level`.
    pub fn shown(&self, now: f64) -> f64 {
        let elapsed = (now - self.start_time).max(0.0);
        if self.level >= self.start {
            // Land exactly on the reading; rounding would otherwise keep `is_moving` true.
            if elapsed >= ATTACK_SECONDS {
                return self.level;
            }
            return self.start + (self.level - self.start) * elapsed / ATTACK_SECONDS;
        }
        (self.start - RELEASE_DB_PER_SECOND * elapsed).max(self.level)
    }

    /// Whether the bar still moves, so the window has to keep drawing.
    pub fn is_moving(&self, now: f64) -> bool {
        self.shown(now) != self.level
    }

    /// The held peak, or the bar once the hold ran out.
    pub fn peak(&self, now: f64) -> f64 {
        if now - self.peak_since >= PEAK_HOLD_SECONDS {
            return self.shown(now);
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
        assert_eq!(meter.peak(1.5), meter.shown(1.5));
    }

    #[test]
    fn meter_fraction_clamps_below_floor_and_above_ceiling() {
        assert_eq!(fraction(-160.0), 0.0);
        assert_eq!(fraction(10.0), 1.0);
        assert_eq!(fraction(-57.0), 0.5);
    }

    #[test]
    fn meter_rises_within_the_attack_time() {
        let mut meter = MeterState::default();
        meter.update(-20.0, 10.0);
        assert_eq!(meter.shown(10.0), METER_FLOOR_DB);
        let halfway = meter.shown(10.0 + ATTACK_SECONDS / 2.0);
        assert!(halfway > METER_FLOOR_DB && halfway < -20.0);
        assert_eq!(meter.shown(10.0 + ATTACK_SECONDS), -20.0);
    }

    #[test]
    fn meter_falls_at_the_release_rate_instead_of_jumping() {
        let mut meter = MeterState::default();
        meter.update(-20.0, 0.0);
        meter.update(-80.0, 1.0);
        assert_eq!(meter.shown(1.0), -20.0);
        assert_eq!(meter.shown(1.5), -20.0 - RELEASE_DB_PER_SECOND * 0.5);
        assert_eq!(meter.shown(10.0), -80.0);
        assert!(meter.is_moving(1.5));
        assert!(!meter.is_moving(10.0));
    }

    #[test]
    fn quiet_room_level_is_visible() {
        // Measured on a KH 120 II at quiet room volume: the input meter reads about -90 dB.
        assert!(fraction(-90.0) > 0.2);
    }
}
