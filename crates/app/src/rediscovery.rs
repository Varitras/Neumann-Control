//! When the app looks for its speakers again without being asked: a link-local address keeps the
//! interface index of the computer, and Windows may give the interface another one after a
//! restart. A silent scan finds a remembered speaker by its serial at the new address.

/// How long a remembered speaker stays unreachable before the app looks for it.
pub const UNREACHABLE_GRACE_SECONDS: f64 = 30.0;
/// The least time between two scans for speakers that stay unreachable.
pub const REPEAT_SECONDS: f64 = 300.0;

#[derive(Debug, Default)]
pub struct Rediscovery {
    started: bool,
    last: Option<f64>,
}

impl Rediscovery {
    /// Whether a silent scan should start now. `followable`: some remembered speaker has a serial
    /// a scan could find it by; `unreachable_for`: how long the longest-missing one has been
    /// unreachable; `scanning`: a scan runs already.
    pub fn due(
        &mut self,
        now: f64,
        followable: bool,
        unreachable_for: Option<f64>,
        scanning: bool,
    ) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_scans_once_for_remembered_speakers() {
        let mut rediscovery = Rediscovery::default();
        assert!(rediscovery.due(0.0, true, None, false));
        assert!(!rediscovery.due(1.0, true, None, false));
    }

    #[test]
    fn nothing_to_follow_means_no_scan() {
        let mut rediscovery = Rediscovery::default();
        assert!(!rediscovery.due(0.0, false, None, false));
        assert!(!rediscovery.due(100.0, false, Some(100.0), false));
    }

    #[test]
    fn an_unreachable_speaker_is_looked_for_after_the_grace() {
        let mut rediscovery = Rediscovery::default();
        rediscovery.due(0.0, true, None, false);
        let almost = UNREACHABLE_GRACE_SECONDS - 0.5;
        assert!(!rediscovery.due(10.0, true, Some(almost), false));
        assert!(rediscovery.due(10.5, true, Some(UNREACHABLE_GRACE_SECONDS), false));
    }

    #[test]
    fn a_speaker_that_stays_away_is_looked_for_at_most_every_repeat() {
        let mut rediscovery = Rediscovery::default();
        rediscovery.due(0.0, true, None, false);
        assert!(rediscovery.due(40.0, true, Some(40.0), false));
        let soon = 40.0 + REPEAT_SECONDS - 1.0;
        assert!(!rediscovery.due(soon, true, Some(soon), false));
        let later = 40.0 + REPEAT_SECONDS;
        assert!(rediscovery.due(later, true, Some(later), false));
    }

    #[test]
    fn never_while_a_scan_runs() {
        let mut rediscovery = Rediscovery::default();
        assert!(!rediscovery.due(0.0, true, None, true));
        assert!(rediscovery.due(1.0, true, None, false), "the start scan comes after");
        assert!(!rediscovery.due(50.0, true, Some(50.0), true));
    }
}
