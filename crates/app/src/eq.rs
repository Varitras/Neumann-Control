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

/// The leaves a reset to flat sets to 0 dB: `gain` is a flat level and `boost` the filter's
/// height (measured), so with both at 0 a band changes nothing whatever its type.
pub const FLATTENING_LEAVES: [Leaf; 2] = [Leaf::Gain, Leaf::Boost];

/// The value that sets every band of a leaf to 0.
pub fn flat(length: usize) -> Value {
    Value::from(vec![0.0; length])
}

/// The value that writes `value` to `band` only (measured: `null` leaves a band as it is).
pub fn one_band(length: usize, band: usize, value: Value) -> Value {
    let mut array = vec![Value::Null; length];
    array[band] = value;
    Value::Array(array)
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
    fn flat_sets_every_band_to_zero() {
        assert_eq!(flat(3), json!([0.0, 0.0, 0.0]));
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
    fn one_band_is_null_elsewhere() {
        assert_eq!(one_band(3, 1, json!(2.5)), json!([null, 2.5, null]));
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
