//! Level meters: the shown level and a peak held for a while. Times are seconds of the UI clock.

use neumann_control_core::settings::MeterFloor;

pub const PEAK_HOLD_SECONDS: f64 = 1.5;
/// A falling peak would make its readout count down every frame; it steps at this pace instead.
pub const READOUT_REFRESH_SECONDS: f64 = 0.25;
/// Low enough that quiet room volume (inputs around -90 dB on the KH 120 II) still moves.
pub const METER_FLOOR_DB: f64 = MeterFloor::Minus120.db();
pub const METER_CEILING_DB: f64 = 6.0;

/// Below this a level is safe, up to `OVER_FROM_DB` hot, above over.
pub const HOT_FROM_DB: f64 = -18.0;
pub const OVER_FROM_DB: f64 = -6.0;

/// The meter's scale: dB against height from -120 to +6 dB. The top is spread out, where a
/// level matters, and the bottom still leaves quiet room volume (about -90 dB) visible.
const SCALE: [(f64, f64); 10] = [
    (METER_FLOOR_DB, 0.0),
    (-90.0, 0.18),
    (-60.0, 0.32),
    (-40.0, 0.42),
    (-20.0, 0.56),
    (-10.0, 0.68),
    (-6.0, 0.76),
    (-3.0, 0.85),
    (0.0, 0.93),
    (METER_CEILING_DB, 1.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Safe,
    Hot,
    Over,
}

pub fn zone(db: f64) -> Zone {
    if db > OVER_FROM_DB {
        return Zone::Over;
    }
    if db > HOT_FROM_DB {
        return Zone::Hot;
    }
    Zone::Safe
}

/// Where `db` sits on the meter, 0 at `floor` (from the desk settings) and 1 at the top. The
/// device reports silence as -160, which lands below any floor and gives 0.
pub fn position(db: f64, floor: f64) -> f32 {
    let bottom = scale_height(floor);
    let height = (scale_height(db) - bottom) / (1.0 - bottom);
    height.clamp(0.0, 1.0) as f32
}

/// The level at a height of the meter; the inverse of `position`.
pub fn level_at(position: f32, floor: f64) -> f64 {
    let bottom = scale_height(floor);
    let height = bottom + f64::from(position) * (1.0 - bottom);
    let pairs = SCALE.windows(2);
    for pair in pairs {
        let [(low_db, low), (high_db, high)] = [pair[0], pair[1]];
        if height <= high {
            return low_db + (height - low) / (high - low) * (high_db - low_db);
        }
    }
    METER_CEILING_DB
}

fn scale_height(db: f64) -> f64 {
    let db = db.clamp(METER_FLOOR_DB, METER_CEILING_DB);
    for pair in SCALE.windows(2) {
        let [(low_db, low), (high_db, high)] = [pair[0], pair[1]];
        if db <= high_db {
            return low + (db - low_db) / (high_db - low_db) * (high - low);
        }
    }
    1.0
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
        // After the hold the peak is the falling bar; only a reading above it holds anew.
        let above_falling_bar = hold_expired && level >= self.start;
        if level >= self.peak || above_falling_bar {
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

    /// The held peak in whole dB for the number under the meter; `None` at or below `floor`.
    /// While the hold lasts it is the held peak itself; afterwards it follows the falling bar
    /// in steps of `READOUT_REFRESH_SECONDS`, so a new peak still shows at once.
    pub fn readout(&self, now: f64, floor: f64) -> Option<i32> {
        let hold_end = self.peak_since + PEAK_HOLD_SECONDS;
        let db = if now < hold_end {
            self.peak
        } else {
            let step = (now / READOUT_REFRESH_SECONDS).floor() * READOUT_REFRESH_SECONDS;
            self.shown(step.max(hold_end))
        };
        if db <= floor {
            return None;
        }
        Some(db.round() as i32)
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
    fn readout_rounds_the_held_peak() {
        let mut meter = MeterState::default();
        meter.update(-18.4, 0.0);
        meter.update(-40.0, 0.5);
        assert_eq!(meter.readout(0.6, -120.0), Some(-18));
    }

    #[test]
    fn readout_rises_at_once_and_steps_while_falling() {
        let mut meter = MeterState::default();
        meter.update(-6.0, 0.0);
        meter.update(-30.0, 1.0);
        assert_eq!(meter.readout(1.4, -120.0), Some(-6));
        // The hold ran out at 1.5 s; the bar falls 24 dB/s from -6 dB at 1.0 s.
        assert_eq!(meter.readout(1.6, -120.0), Some(-18));
        assert_eq!(meter.readout(1.7, -120.0), Some(-18));
        assert_eq!(meter.readout(1.75, -120.0), Some(-24));
        meter.update(-3.0, 1.8);
        assert_eq!(meter.readout(1.8, -120.0), Some(-3));
    }

    /// Playback stopped: the speaker reports silence ten times a second while the bar still
    /// falls; the number and the peak line follow the bar instead of the silence.
    #[test]
    fn after_the_hold_the_peak_follows_the_bar_while_silence_is_reported() {
        let mut meter = MeterState::default();
        meter.update(-6.0, 0.0);
        for tenth in 1..=16 {
            meter.update(-160.0, f64::from(tenth) / 10.0);
        }
        assert_eq!(meter.peak(1.6), meter.shown(1.6));
        assert!(meter.readout(1.6, -120.0).is_some());
    }

    #[test]
    fn readout_is_none_below_the_floor() {
        let mut meter = MeterState::default();
        meter.update(-160.0, 0.0);
        assert_eq!(meter.readout(0.1, -90.0), None);
        meter.update(-95.0, 1.0);
        assert_eq!(meter.readout(1.1, -90.0), None);
        assert_eq!(meter.readout(1.1, -120.0), Some(-95));
    }

    #[test]
    fn position_clamps_below_floor_and_above_ceiling() {
        assert_eq!(position(-160.0, METER_FLOOR_DB), 0.0);
        assert_eq!(position(10.0, METER_FLOOR_DB), 1.0);
        assert_eq!(position(METER_CEILING_DB, METER_FLOOR_DB), 1.0);
    }

    #[test]
    fn the_top_of_the_scale_is_spread_out() {
        // 0 to -10 dB takes more room than -60 to -70 dB.
        let top = position(0.0, METER_FLOOR_DB) - position(-10.0, METER_FLOOR_DB);
        let low = position(-60.0, METER_FLOOR_DB) - position(-70.0, METER_FLOOR_DB);
        assert!(top > 2.0 * low, "top {top}, low {low}");
    }

    #[test]
    fn level_at_is_the_inverse_of_position() {
        for db in [-110.0, -90.0, -40.0, -18.0, -6.0, -1.0, 3.0] {
            let back = level_at(position(db, METER_FLOOR_DB), METER_FLOOR_DB);
            assert!((back - db).abs() < 0.01, "{db} -> {back}");
        }
    }

    #[test]
    fn zones_split_at_minus_18_and_minus_6() {
        assert_eq!(zone(-30.0), Zone::Safe);
        assert_eq!(zone(-18.0), Zone::Safe);
        assert_eq!(zone(-10.0), Zone::Hot);
        assert_eq!(zone(-6.0), Zone::Hot);
        assert_eq!(zone(-1.0), Zone::Over);
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
        assert!(position(-90.0, METER_FLOOR_DB) > 0.15);
    }

    #[test]
    fn meter_floor_setting_changes_the_scale() {
        assert_eq!(position(-60.0, -60.0), 0.0);
        assert_eq!(position(-90.0, -60.0), 0.0);
        assert!(position(-60.0, -120.0) > 0.25);
        // A higher floor gives the bottom of the remaining range more room.
        let near_floor = |floor: f64| position(-55.0, floor) - position(-60.0, floor);
        assert!(near_floor(-60.0) > near_floor(-120.0));
    }
}
