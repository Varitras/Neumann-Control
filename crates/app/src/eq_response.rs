//! The magnitude response of EQ bands, computed from their settings (not measured): analog
//! prototypes of the usual filter shapes, so no sample rate has to be assumed.

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
    let Some((scale, numerator, denominator)) = transfer(band) else {
        return 0.0;
    };
    let magnitude = scale * numerator.magnitude(x) / denominator.magnitude(x);
    20.0 * magnitude.log10()
}

/// Numerator and denominator of the shape (RBJ audio EQ cookbook, analog form), with a factor.
fn transfer(band: &Band) -> Option<(f64, Quadratic, Quadratic)> {
    let q = band.q;
    let a = 10f64.powf(band.effective_gain() / 40.0);
    let resonant = Quadratic::new(1.0, 1.0 / q, 1.0);
    let first_order = Quadratic::new(0.0, 1.0, 1.0);
    let shelf_slope = a.sqrt() / q;
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
        "BANDPASS" => (1.0, Quadratic::new(0.0, 1.0 / q, 0.0), resonant),
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

    fn band(kind: &str, gain: f64, frequency: f64, q: f64) -> Band {
        Band {
            enabled: true,
            gain,
            boost: 0.0,
            frequency,
            q,
            kind: kind.to_owned(),
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 0.05, "{actual} vs {expected}");
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
        close(band_db(&band("BANDPASS", 0.0, 1000.0, 2.0), 1000.0), 0.0);
    }

    #[test]
    fn phase_only_and_unknown_types_are_flat() {
        for kind in ["ALLPASS", "INVERSION", "SOMETHING"] {
            close(band_db(&band(kind, 6.0, 1000.0, 1.0), 1000.0), 0.0);
        }
    }

    #[test]
    fn boost_adds_to_gain() {
        let mut boosted = band("PARAMETRIC", 3.0, 1000.0, 1.0);
        boosted.boost = 3.0;
        close(band_db(&boosted, 1000.0), 6.0);
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
