//! EQ bands as the speaker reports them: six arrays per container, one entry per band. A write
//! changes one leaf of one band and leaves the others alone (`null` elsewhere).

use std::collections::BTreeMap;

use neumann_control_core::catalog::{Catalog, EqContainer};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::state::SpeakerState;

/// The six leaves of an EQ container, as the catalog names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaf {
    Enabled,
    Gain,
    Boost,
    Frequency,
    Q,
    Type,
}

impl Leaf {
    pub const ALL: [Leaf; 6] = [
        Leaf::Enabled,
        Leaf::Gain,
        Leaf::Boost,
        Leaf::Frequency,
        Leaf::Q,
        Leaf::Type,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Leaf::Enabled => "enabled",
            Leaf::Gain => "gain",
            Leaf::Boost => "boost",
            Leaf::Frequency => "frequency",
            Leaf::Q => "q",
            Leaf::Type => "type",
        }
    }

    pub fn path(self, container: &SscPath) -> SscPath {
        container.child(self.name())
    }
}

/// One band of a container.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub enabled: bool,
    pub gain: f64,
    pub boost: f64,
    pub frequency: f64,
    pub q: f64,
    pub kind: String,
}

/// The bands of `container`; `None` unless all six leaves are reported as arrays of one length
/// with the right element types, so a strange report is shown read-only and never written.
pub fn bands_of(values: &BTreeMap<SscPath, Value>, container: &SscPath) -> Option<Vec<Band>> {
    let array = |leaf: Leaf| values.get(&leaf.path(container))?.as_array();
    let numbers =
        |leaf: Leaf| -> Option<Vec<f64>> { array(leaf)?.iter().map(Value::as_f64).collect() };
    let enabled: Vec<bool> = array(Leaf::Enabled)?
        .iter()
        .map(Value::as_bool)
        .collect::<Option<_>>()?;
    let kinds: Vec<String> = array(Leaf::Type)?
        .iter()
        .map(|kind| kind.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    let (gain, boost) = (numbers(Leaf::Gain)?, numbers(Leaf::Boost)?);
    let (frequency, q) = (numbers(Leaf::Frequency)?, numbers(Leaf::Q)?);
    let count = enabled.len();
    let lengths = [
        kinds.len(),
        gain.len(),
        boost.len(),
        frequency.len(),
        q.len(),
    ];
    if lengths.iter().any(|length| *length != count) {
        return None;
    }
    let bands = (0..count)
        .map(|band| Band {
            enabled: enabled[band],
            gain: gain[band],
            boost: boost[band],
            frequency: frequency[band],
            q: q[band],
            kind: kinds[band].clone(),
        })
        .collect();
    Some(bands)
}

/// Gain and boost move in half decibels, Q in tenths, frequencies keep three digits.
pub const GAIN_GRID_DB: f64 = 0.5;
pub const Q_STEP: f64 = 0.1;
const FREQUENCY_DIGITS: i32 = 3;

/// `frequency` rounded to `FREQUENCY_DIGITS` significant digits (1230 Hz, 47.3 Hz).
pub fn snapped_frequency(frequency: f64) -> f64 {
    let magnitude = frequency.abs().log10().floor() as i32;
    let unit = 10f64.powi(magnitude + 1 - FREQUENCY_DIGITS);
    let snapped = (frequency / unit).round() * unit;
    // Rounding by a power of ten leaves binary noise (47.300000000000004).
    let decimals = (FREQUENCY_DIGITS - 1 - magnitude).max(0);
    let scale = 10f64.powi(decimals);
    (snapped * scale).round() / scale
}

/// The EQ container a band leaf belongs to.
pub fn container_of<'a>(catalog: &'a Catalog, path: &SscPath) -> Option<&'a EqContainer> {
    catalog.eq.iter().find(|container| {
        Leaf::ALL
            .iter()
            .any(|leaf| leaf.path(&container.path) == *path)
    })
}

/// Whether the speaker has an EQ to open: only once it reported its model, because containers
/// of the class "all" match any product, an unknown one too.
pub fn has_eq(catalog: &Catalog, speaker: &SpeakerState) -> bool {
    speaker.identity.is_some() && !catalog.eq_containers(speaker.product()).is_empty()
}

/// What a full reset sets every band to: what the tested speakers report for bands nobody
/// touched, not a measured factory state.
const RESET_TYPE: &str = "PARAMETRIC";
const RESET_FREQUENCY_HZ: f64 = 100.0;
const RESET_Q: f64 = 0.7;

/// Setting every band of a container back at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetKind {
    /// `gain` and `boost` to 0 dB: `gain` is a flat level and `boost` the filter's height
    /// (measured), so with both at 0 a band changes nothing whatever its type. Bands stay on.
    Flat,
    /// Every band off and every leaf back to the start values.
    Full,
}

impl ResetKind {
    /// In the order they are written: a full reset switches the bands off first, so the other
    /// leaves change while nothing sounds.
    pub fn leaves(self) -> &'static [Leaf] {
        match self {
            ResetKind::Flat => &[Leaf::Gain, Leaf::Boost],
            ResetKind::Full => &Leaf::ALL,
        }
    }

    /// The whole array a leaf of `length` bands gets.
    pub fn value(self, leaf: Leaf, length: usize) -> Value {
        let entry = match leaf {
            Leaf::Enabled => Value::Bool(false),
            Leaf::Type => Value::from(RESET_TYPE),
            Leaf::Frequency => Value::from(RESET_FREQUENCY_HZ),
            Leaf::Q => Value::from(RESET_Q),
            Leaf::Gain | Leaf::Boost => Value::from(0.0),
        };
        Value::Array(vec![entry; length])
    }
}

/// The value that writes `value` to `band` only (measured: `null` leaves a band as it is).
/// `None` for a band the container does not have (the speaker reported fewer bands).
pub fn one_band(length: usize, band: usize, value: Value) -> Option<Value> {
    let mut array = vec![Value::Null; length];
    *array.get_mut(band)? = value;
    Some(Value::Array(array))
}

/// The output a container belongs to: `out`, `out1` or `out2`.
pub fn output_key(container: &EqContainer) -> String {
    container
        .path
        .segments()
        .get(1)
        .cloned()
        .unwrap_or_default()
}

/// One output's response for each input that has containers of its own (the KH 750's
/// crossovers): the containers every input passes plus that input's. The crossovers lie side by
/// side, one per input, so adding them all would describe a signal no input takes. Without
/// input-specific containers there is one sum, named `None`. `curves` are in dB per frequency.
pub fn input_sums(
    curves: &[(Option<&str>, &[f64])],
    points: usize,
) -> Vec<(Option<String>, Vec<f64>)> {
    let mut inputs: Vec<&str> = curves.iter().filter_map(|(input, _)| *input).collect();
    inputs.dedup();
    let sum_for = |input: Option<&str>| -> Vec<f64> {
        let passes = |own: &Option<&str>| own.is_none() || *own == input;
        (0..points)
            .map(|point| {
                curves
                    .iter()
                    .filter(|(own, _)| passes(own))
                    .map(|(_, curve)| curve[point])
                    .sum()
            })
            .collect()
    };
    if inputs.is_empty() {
        return vec![(None, sum_for(None))];
    }
    inputs
        .into_iter()
        .map(|input| (Some(input.to_owned()), sum_for(Some(input))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::paths;

    fn container() -> SscPath {
        paths::path("audio/out/eq2")
    }

    fn values(entries: &[(&str, Value)]) -> BTreeMap<SscPath, Value> {
        entries
            .iter()
            .map(|(leaf, value)| (container().child(leaf), value.clone()))
            .collect()
    }

    #[test]
    fn flat_sets_level_and_boost_of_every_band_to_zero() {
        assert_eq!(ResetKind::Flat.leaves(), &[Leaf::Gain, Leaf::Boost]);
        assert_eq!(ResetKind::Flat.value(Leaf::Gain, 3), json!([0.0, 0.0, 0.0]));
    }

    #[test]
    fn a_full_reset_switches_off_first_and_sets_every_leaf() {
        let leaves = ResetKind::Full.leaves();
        assert_eq!(leaves.len(), 6);
        assert_eq!(leaves[0], Leaf::Enabled);
        let value = |leaf| ResetKind::Full.value(leaf, 2);
        assert_eq!(value(Leaf::Enabled), json!([false, false]));
        assert_eq!(value(Leaf::Type), json!(["PARAMETRIC", "PARAMETRIC"]));
        assert_eq!(value(Leaf::Frequency), json!([100.0, 100.0]));
        assert_eq!(value(Leaf::Q), json!([0.7, 0.7]));
        assert_eq!(value(Leaf::Gain), json!([0.0, 0.0]));
        assert_eq!(value(Leaf::Boost), json!([0.0, 0.0]));
    }

    fn two_bands() -> BTreeMap<SscPath, Value> {
        values(&[
            ("enabled", json!([true, false])),
            ("gain", json!([3.0, 0.0])),
            ("boost", json!([1.0, 0.0])),
            ("frequency", json!([100.0, 1000.0])),
            ("q", json!([0.7, 1.0])),
            ("type", json!(["PARAMETRIC", "NOTCH"])),
        ])
    }

    #[test]
    fn bands_of_reads_six_arrays() {
        let bands = bands_of(&two_bands(), &container()).unwrap();
        assert_eq!(bands.len(), 2);
        assert_eq!(
            bands[0],
            Band {
                enabled: true,
                gain: 3.0,
                boost: 1.0,
                frequency: 100.0,
                q: 0.7,
                kind: "PARAMETRIC".to_owned(),
            }
        );
        assert_eq!(bands[1].kind, "NOTCH");
    }

    #[test]
    fn bands_of_refuses_mixed_lengths_and_wrong_types() {
        let mut values = two_bands();
        values.insert(container().child("q"), json!([0.7]));
        assert_eq!(bands_of(&values, &container()), None);
        let mut values = two_bands();
        values.insert(container().child("gain"), json!([3.0, "x"]));
        assert_eq!(bands_of(&values, &container()), None);
        let mut values = two_bands();
        values.remove(&container().child("type"));
        assert_eq!(bands_of(&values, &container()), None);
    }

    #[test]
    fn the_crossovers_of_two_inputs_are_summed_apart() {
        let shared = [1.0, 2.0];
        let crossover_a = [-10.0, 0.0];
        let crossover_b = [0.0, -20.0];
        let curves = [
            (None, &shared[..]),
            (Some("A"), &crossover_a[..]),
            (Some("B"), &crossover_b[..]),
        ];
        assert_eq!(
            input_sums(&curves, 2),
            vec![
                (Some("A".to_owned()), vec![-9.0, 2.0]),
                (Some("B".to_owned()), vec![1.0, -18.0]),
            ]
        );
    }

    #[test]
    fn containers_without_an_input_give_one_sum() {
        let first = [1.0, 2.0];
        let second = [3.0, 4.0];
        let curves = [(None, &first[..]), (None, &second[..])];
        assert_eq!(input_sums(&curves, 2), vec![(None, vec![4.0, 6.0])]);
    }

    #[test]
    fn one_band_is_null_elsewhere() {
        assert_eq!(one_band(3, 1, json!(2.5)), Some(json!([null, 2.5, null])));
        assert_eq!(one_band(3, 3, json!(2.5)), None);
    }

    #[test]
    fn a_frequency_keeps_three_digits() {
        assert_eq!(snapped_frequency(1234.5), 1230.0);
        assert_eq!(snapped_frequency(99.95), 100.0);
        assert_eq!(snapped_frequency(47.26), 47.3);
        assert_eq!(snapped_frequency(23999.0), 24000.0);
    }

    #[test]
    fn a_band_path_finds_its_container() {
        let catalog = neumann_control_core::catalog::Catalog::embedded();
        let found = container_of(catalog, &paths::path("audio/out/eq3/q")).unwrap();
        assert_eq!(found.path, paths::path("audio/out/eq3"));
        assert!(container_of(catalog, &paths::path("audio/out/level")).is_none());
    }

    #[test]
    fn a_leaf_names_its_path() {
        assert_eq!(
            Leaf::Gain.path(&container()),
            paths::path("audio/out/eq2/gain")
        );
        assert_eq!(Leaf::ALL.len(), 6);
    }
}
