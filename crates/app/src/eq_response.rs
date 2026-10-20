//! The magnitude response of EQ bands, computed from their settings (not measured): analog
//! prototypes of the usual filter shapes, so no sample rate has to be assumed. Measured on the
//! KH 750: a band's `gain` lifts every frequency, its `boost` is the height of the filter.

use crate::eq::Band;

/// The graph's frequency axis: the range the devices accept for `frequency`.
pub const FREQUENCY_MIN: f64 = 10.0;
pub const FREQUENCY_MAX: f64 = 24000.0;
pub const GRAPH_POINTS: usize = 400;

/// `a2·s² + a1·s + a0` at `s = j·x`, where `x` is the frequency over the band's frequency.
#[derive(Clone, Copy)]
struct Quadratic {
    a2: f64,
    a1: f64,
    a0: f64,
}

impl Quadratic {
    const fn new(a2: f64, a1: f64, a0: f64) -> Quadratic {
        Quadratic { a2, a1, a0 }
    }

    fn magnitude(self, x: f64) -> f64 {
        let real = self.a0 - self.a2 * x * x;
        let imaginary = self.a1 * x;
        real.hypot(imaginary)
    }
}

/// The band's level change at `frequency`, in dB.
pub fn band_db(band: &Band, frequency: f64) -> f64 {
    if !band.enabled {
        return 0.0;
    }
    let x = frequency / band.frequency;
    let shape = transfer(band).map_or(0.0, |(scale, numerator, denominator)| {
        let magnitude = scale * numerator.magnitude(x) / denominator.magnitude(x);
        20.0 * magnitude.log10()
    });
    band.gain + shape
}

/// Numerator and denominator of the shape (RBJ audio EQ cookbook, analog form), with a factor.
fn transfer(band: &Band) -> Option<(f64, Quadratic, Quadratic)> {
    let q = band.q;
    let a = 10f64.powf(band.boost / 40.0);
    let resonant = Quadratic::new(1.0, 1.0 / q, 1.0);
    let first_order = Quadratic::new(0.0, 1.0, 1.0);
    // Measured on the KH 120 II: a shelf's `q` is the cookbook's slope S, not its Q.
    let shelf_radicand = (a + 1.0 / a) * (1.0 / q - 1.0) + 2.0;
    let is_shelf = matches!(band.kind.as_str(), "LOSHELF" | "HISHELF");
    if is_shelf && shelf_radicand <= 0.0 {
        // A slope this steep for this height is outside the cookbook form; not measured.
        return None;
    }
    let shelf_slope = a.sqrt() * shelf_radicand.max(0.0).sqrt();
    let shape = match band.kind.as_str() {
        "PARAMETRIC" => (
            1.0,
            Quadratic::new(1.0, a / q, 1.0),
            Quadratic::new(1.0, 1.0 / (a * q), 1.0),
        ),
        "LOSHELF" => (
            a,
            Quadratic::new(1.0, shelf_slope, a),
            Quadratic::new(a, shelf_slope, 1.0),
        ),
        "HISHELF" => (
            a,
            Quadratic::new(a, shelf_slope, 1.0),
            Quadratic::new(1.0, shelf_slope, a),
        ),
        "LOWPASS" => (1.0, Quadratic::new(0.0, 0.0, 1.0), resonant),
        "HIGHPASS" => (1.0, Quadratic::new(1.0, 0.0, 0.0), resonant),
        // Measured: the centre is 20·log10(Q) dB, the constant-skirt form.
        "BANDPASS" => (1.0, Quadratic::new(0.0, 1.0, 0.0), resonant),
        "NOTCH" => (1.0, Quadratic::new(1.0, 0.0, 1.0), resonant),
        "LO6DB" => (1.0, Quadratic::new(0.0, 0.0, 1.0), first_order),
        "HI6DB" => (1.0, Quadratic::new(0.0, 1.0, 0.0), first_order),
        // ALLPASS and INVERSION change the phase only; an unknown type is drawn flat.
        _ => return None,
    };
    Some(shape)
}

/// The sum of the bands at each frequency, in dB.
pub fn curve_db(bands: &[Band], frequencies: &[f64]) -> Vec<f64> {
    frequencies
        .iter()
        .map(|frequency| bands.iter().map(|band| band_db(band, *frequency)).sum())
        .collect()
}

/// Each band on its own, to show what it does apart from the others; `None` for a band that is
/// off.
pub fn band_curves(bands: &[Band], frequencies: &[f64]) -> Vec<Option<Vec<f64>>> {
    bands
        .iter()
        .map(|band| {
            band.enabled
                .then(|| curve_db(std::slice::from_ref(band), frequencies))
        })
        .collect()
}

/// `GRAPH_POINTS` frequencies spaced evenly on a log scale from the lowest to the highest.
pub fn graph_frequencies() -> Vec<f64> {
    let (low, high) = (FREQUENCY_MIN.ln(), FREQUENCY_MAX.ln());
    let last = (GRAPH_POINTS - 1) as f64;
    (0..GRAPH_POINTS)
        .map(|point| (low + (high - low) * point as f64 / last).exp())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_1_SQRT_2;

    use super::*;

    /// A band with `boost`, the filter's height; its flat `gain` is 0.
    fn band(kind: &str, boost: f64, frequency: f64, q: f64) -> Band {
        Band {
            enabled: true,
            gain: 0.0,
            boost,
            frequency,
            q,
            kind: kind.to_owned(),
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 0.05, "{actual} vs {expected}");
    }

    #[test]
    fn the_chosen_band_alone_reaches_its_own_height() {
        let bands = [
            band("PARAMETRIC", -15.0, 21.0, 0.7),
            band("PARAMETRIC", 10.5, 51.9, 1.4),
        ];
        let frequencies = [51.9];
        let own = band_curves(&bands, &frequencies);
        close(own[1].as_ref().unwrap()[0], 10.5);
        assert!(curve_db(&bands, &frequencies)[0] < 10.0);
        let mut off = bands.clone();
        off[1].enabled = false;
        assert!(band_curves(&off, &frequencies)[1].is_none());
        assert_eq!(band_curves(&bands, &frequencies).len(), 2);
    }

    #[test]
    fn peaking_reaches_its_gain_at_its_frequency() {
        close(band_db(&band("PARAMETRIC", 6.0, 1000.0, 1.0), 1000.0), 6.0);
        close(
            band_db(&band("PARAMETRIC", -9.0, 1000.0, 4.0), 1000.0),
            -9.0,
        );
    }

    #[test]
    fn peaking_is_flat_far_away() {
        assert!(band_db(&band("PARAMETRIC", 6.0, 1000.0, 1.0), 10.0).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_their_gain_at_their_end() {
        close(band_db(&band("LOSHELF", 6.0, 200.0, 0.7), 10.0), 6.0);
        close(band_db(&band("HISHELF", -4.0, 2000.0, 0.7), 24000.0), -4.0);
    }

    /// How far a computed value may lie from a measured one: the measurements (REW, UMIK-1,
    /// 1/12 octave, each against the band switched off) scatter by about 0.1 dB.
    const MEASURED_TOLERANCE_DB: f64 = 0.3;

    /// `band`'s value at each frequency against what a KH 120 II measured there.
    fn as_measured(band: &Band, measured: &[(f64, f64)]) {
        for (frequency, expected) in measured {
            let actual = band_db(band, *frequency);
            let off = (actual - expected).abs();
            assert!(
                off < MEASURED_TOLERANCE_DB,
                "{} at {frequency} Hz: {actual:.2} vs measured {expected}",
                band.kind
            );
        }
    }

    /// Measured on a KH 120 II: the speakers' `q` of a shelf is the cookbook's slope S, so
    /// q 2.0 overshoots only slightly.
    #[test]
    fn a_shelf_q_of_2_overshoots_as_little_as_measured() {
        let near = |actual: f64, expected: f64| {
            assert!(
                (actual - expected).abs() < MEASURED_TOLERANCE_DB,
                "{actual} vs {expected}"
            );
        };
        let low = band("LOSHELF", 6.0, 200.0, 2.0);
        near(band_db(&low, 100.0), 6.54);
        near(band_db(&low, 400.0), -0.50);
        let high = band("HISHELF", 6.0, 2000.0, 2.0);
        near(band_db(&high, 1000.0), -0.53);
        near(band_db(&high, 5657.0), 6.37);
        let steepest_without_overshoot = band("LOSHELF", 6.0, 200.0, 1.0);
        near(band_db(&steepest_without_overshoot, 100.0), 5.70);
        near(band_db(&steepest_without_overshoot, 400.0), 0.41);
    }

    /// Measured on a KH 120 II: the EQ runs at 48 kHz, so near the top a band is narrower than
    /// its analog form (a 14 kHz peak is 2.7 dB at 10 kHz, not 4).
    #[test]
    fn filters_near_the_top_are_as_narrow_as_measured() {
        as_measured(
            &band("PARAMETRIC", 6.0, 14000.0, 1.0),
            &[
                (7000.0, 1.02),
                (10000.0, 2.72),
                (14000.0, 5.93),
                (16000.0, 4.67),
            ],
        );
        as_measured(
            &band("HISHELF", 6.0, 12000.0, 0.7),
            &[
                (6000.0, 0.56),
                (10000.0, 2.0),
                (14000.0, 3.99),
                (16000.0, 4.82),
            ],
        );
    }

    /// Measured on a KH 120 II: `q` is the cookbook Q for these types, the 6 dB filters are first
    /// order, the band pass peaks at 20·log10(Q).
    #[test]
    fn every_filter_type_is_drawn_as_measured() {
        as_measured(
            &band("PARAMETRIC", 6.0, 1000.0, 4.0),
            &[(1000.0, 5.83), (1260.0, 1.37), (2000.0, 0.2)],
        );
        as_measured(
            &band("LOWPASS", 0.0, 1000.0, FRAC_1_SQRT_2),
            &[(500.0, -0.28), (1000.0, -3.01), (2000.0, -12.34)],
        );
        as_measured(
            &band("HIGHPASS", 0.0, 1000.0, 1.0),
            &[(500.0, -11.03), (1000.0, -0.04), (2000.0, 0.92)],
        );
        as_measured(
            &band("LO6DB", 0.0, 1000.0, 0.7),
            &[(500.0, -0.98), (1000.0, -3.02), (2000.0, -7.01)],
        );
        as_measured(
            &band("HI6DB", 0.0, 1000.0, 0.7),
            &[(500.0, -6.91), (1000.0, -3.01), (2000.0, -0.95)],
        );
        as_measured(
            &band("NOTCH", 0.0, 1000.0, 2.0),
            &[(700.0, -1.83), (1400.0, -1.85)],
        );
        as_measured(
            &band("BANDPASS", 0.0, 1000.0, 2.0),
            &[(250.0, -11.56), (1000.0, 5.98), (4000.0, -11.73)],
        );
    }

    #[test]
    fn a_shelf_steeper_than_the_cookbook_form_allows_is_drawn_flat() {
        close(band_db(&band("HISHELF", 12.0, 2000.0, 16.0), 20000.0), 0.0);
    }

    #[test]
    fn low_pass_is_minus_3_db_at_its_frequency_with_q_0_707() {
        close(
            band_db(&band("LOWPASS", 0.0, 80.0, FRAC_1_SQRT_2), 80.0),
            -3.01,
        );
        close(
            band_db(&band("HIGHPASS", 0.0, 80.0, FRAC_1_SQRT_2), 80.0),
            -3.01,
        );
    }

    #[test]
    fn first_order_filters_are_minus_3_db_at_their_frequency() {
        close(band_db(&band("LO6DB", 0.0, 500.0, 1.0), 500.0), -3.01);
        close(band_db(&band("HI6DB", 0.0, 500.0, 1.0), 500.0), -3.01);
    }

    #[test]
    fn notch_and_band_pass() {
        assert!(band_db(&band("NOTCH", 0.0, 1000.0, 2.0), 1000.0) < -60.0);
        // Measured on the KH 750: the band pass peaks at 20·log10(Q), +6 dB for Q 2.
        close(band_db(&band("BANDPASS", 0.0, 50.0, 2.0), 50.0), 6.02);
        close(band_db(&band("BANDPASS", 0.0, 50.0, 2.0), 200.0), -11.56);
    }

    #[test]
    fn phase_only_and_unknown_types_are_flat() {
        for kind in ["ALLPASS", "INVERSION", "SOMETHING"] {
            close(band_db(&band(kind, 6.0, 1000.0, 1.0), 1000.0), 0.0);
        }
    }

    #[test]
    fn gain_lifts_every_frequency() {
        let mut lifted = band("PARAMETRIC", 0.0, 1000.0, 1.0);
        lifted.gain = 6.0;
        close(band_db(&lifted, 10.0), 6.0);
        close(band_db(&lifted, 20000.0), 6.0);
        let mut low_pass = band("LOWPASS", 0.0, 200.0, FRAC_1_SQRT_2);
        low_pass.gain = 6.0;
        close(band_db(&low_pass, 20.0), 6.0);
    }

    #[test]
    fn gain_and_boost_add_at_the_band_frequency() {
        let mut both = band("PARAMETRIC", 3.0, 1000.0, 1.0);
        both.gain = 3.0;
        close(band_db(&both, 1000.0), 6.0);
        close(band_db(&both, 10.0), 3.0);
    }

    #[test]
    fn a_disabled_band_is_flat() {
        let mut off = band("PARAMETRIC", 6.0, 1000.0, 1.0);
        off.enabled = false;
        assert_eq!(band_db(&off, 1000.0), 0.0);
    }

    #[test]
    fn a_curve_is_the_sum_of_its_bands() {
        let bands = [
            band("PARAMETRIC", 6.0, 1000.0, 1.0),
            band("PARAMETRIC", 3.0, 1000.0, 1.0),
        ];
        close(curve_db(&bands, &[1000.0])[0], 9.0);
        let frequencies = graph_frequencies();
        assert_eq!(frequencies.len(), GRAPH_POINTS);
        close(frequencies[0], FREQUENCY_MIN);
        close(*frequencies.last().unwrap(), FREQUENCY_MAX);
    }
}
