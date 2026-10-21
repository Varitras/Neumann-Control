use neumann_control_core::catalog::{Catalog, CatalogError, Conversion, Entry, Kind, ValueError};
use neumann_control_core::settings::Language;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::json;

const MONITOR: &str = "KH 120 II";
const SUBWOOFER: &str = "KH 750";
const UNKNOWN_PRODUCT: &str = "KH 80";

fn path(text: &str) -> SscPath {
    SscPath::parse(text).unwrap()
}

fn entry(text: &str) -> &'static Entry {
    let wanted = path(text);
    Catalog::embedded()
        .entries
        .iter()
        .find(|candidate| candidate.path == wanted)
        .unwrap_or_else(|| panic!("{text} is not in the catalog"))
}

fn has(product: &str, text: &str) -> bool {
    let wanted = path(text);
    Catalog::embedded()
        .entries_for(product)
        .iter()
        .any(|candidate| candidate.path == wanted)
}

#[test]
fn embedded_catalog_loads() {
    assert!(!Catalog::embedded().entries.is_empty());
}

#[test]
fn paths_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for candidate in &Catalog::embedded().entries {
        assert!(seen.insert(&candidate.path), "{} twice", candidate.path);
    }
}

#[test]
fn writable_models_have_the_path() {
    let catalog = Catalog::embedded();
    for candidate in &catalog.entries {
        for product in &candidate.writable_on {
            assert!(
                catalog.entries_for(product).contains(&candidate),
                "{} is writable on {product} but not offered there",
                candidate.path
            );
        }
    }
}

#[test]
fn kh750_has_no_phase_inversion_and_has_out1() {
    assert!(!has(SUBWOOFER, "audio/out/phaseinversion"));
    assert!(has(SUBWOOFER, "audio/out1/level"));
    assert!(has(MONITOR, "audio/out/phaseinversion"));
    assert!(!has(MONITOR, "audio/out1/level"));
}

#[test]
fn kh120ii_has_logo_but_kh750_not() {
    assert!(has(MONITOR, "ui/logo/brightness"));
    assert!(!has(SUBWOOFER, "ui/logo/brightness"));
}

#[test]
fn unknown_model_is_read_only() {
    let entries = &Catalog::embedded().entries;
    // Without a writable entry for a known model the loop below would prove nothing.
    assert!(entries
        .iter()
        .any(|candidate| candidate.is_writable(MONITOR)));
    for candidate in entries {
        assert!(
            !candidate.is_writable(UNKNOWN_PRODUCT),
            "{}",
            candidate.path
        );
    }
}

#[test]
fn auto_standby_switch_is_writable_on_the_monitor_only() {
    let switch = entry("device/standby/enabled");
    assert!(switch.is_writable(MONITOR));
    assert!(!switch.is_writable(SUBWOOFER));
}

#[test]
fn level_out_of_range_is_refused() {
    let catalog = Catalog::embedded();
    let result = entry("audio/out/level").validate(&json!(120.5), MONITOR, &catalog.classes);
    assert_eq!(
        result,
        Err(ValueError::OutOfRange {
            min: 0.0,
            max: 120.0
        })
    );
    assert_eq!(
        entry("audio/out/level").validate(&json!(120), MONITOR, &catalog.classes),
        Ok(())
    );
}

#[test]
fn delay_max_depends_on_model() {
    let catalog = Catalog::embedded();
    let delay = entry("audio/out/delay");
    assert_eq!(delay.max_for(MONITOR, &catalog.classes), Some(5760.0));
    assert_eq!(delay.max_for(SUBWOOFER, &catalog.classes), Some(1000.0));
    assert_eq!(
        delay.validate(&json!(5760), MONITOR, &catalog.classes),
        Ok(())
    );
    assert!(delay
        .validate(&json!(1001), SUBWOOFER, &catalog.classes)
        .is_err());
}

#[test]
fn name_with_umlaut_is_refused() {
    let catalog = Catalog::embedded();
    let result =
        entry("device/name").validate(&json!("Lautsprecher Küche"), MONITOR, &catalog.classes);
    assert_eq!(result, Err(ValueError::NotPrintableAscii));
}

#[test]
fn name_longer_than_52_is_refused() {
    let catalog = Catalog::embedded();
    let name = entry("device/name");
    let longest = "a".repeat(52);
    assert_eq!(
        name.validate(&json!(longest), MONITOR, &catalog.classes),
        Ok(())
    );
    let too_long = "a".repeat(53);
    assert_eq!(
        name.validate(&json!(too_long), MONITOR, &catalog.classes),
        Err(ValueError::TooLong(52))
    );
}

#[test]
fn unknown_option_is_refused() {
    let catalog = Catalog::embedded();
    let interface = entry("audio/in/interface");
    assert_eq!(
        interface.validate(&json!("REMOTE"), MONITOR, &catalog.classes),
        Err(ValueError::NotAnOption)
    );
    assert_eq!(
        interface.validate(&json!("DIGITAL ONLY"), MONITOR, &catalog.classes),
        Ok(())
    );
}

#[test]
fn control_mode_is_read_only_on_every_model() {
    let mode = entry("ui/control_mode");
    assert_eq!(mode.kind, Kind::Display);
    for product in [MONITOR, SUBWOOFER] {
        assert!(!mode.is_writable(product), "{product}");
    }
}

#[test]
fn wrong_json_type_is_refused() {
    let catalog = Catalog::embedded();
    assert_eq!(
        entry("audio/out/mute").validate(&json!(1), MONITOR, &catalog.classes),
        Err(ValueError::WrongType)
    );
    assert_eq!(
        entry("audio/out/level").validate(&json!("loud"), MONITOR, &catalog.classes),
        Err(ValueError::WrongType)
    );
    assert_eq!(
        entry("device/temperature").validate(&json!(300), SUBWOOFER, &catalog.classes),
        Err(ValueError::WrongType)
    );
}

#[test]
fn temperature_is_converted_from_kelvin() {
    let temperature = entry("device/temperature");
    assert_eq!(temperature.unit.as_deref(), Some("K"));
    assert_eq!(temperature.conversion, Some(Conversion::KelvinToCelsius));
    let celsius = Conversion::KelvinToCelsius.apply(313.0);
    assert!((celsius - 39.85).abs() < 1e-9, "{celsius}");
}

#[test]
fn duplicate_path_in_text_is_an_error() {
    let text = r#"
        [models]
        subwoofer = ["KH 750"]
        logo = []

        [[entry]]
        path = "audio/out/mute"
        label = "Stumm"
        label_en = "Mute"
        group = "output"
        kind = "switch"
        poll = "fast"
        class = "all"

        [[entry]]
        path = "audio/out/mute"
        label = "Stumm"
        label_en = "Mute"
        group = "output"
        kind = "switch"
        poll = "fast"
        class = "all"
    "#;
    match Catalog::parse(text) {
        Err(CatalogError::DuplicatePath(duplicate)) => {
            assert_eq!(duplicate, path("audio/out/mute"))
        }
        other => panic!("expected DuplicatePath, got {other:?}"),
    }
}

#[test]
fn select_without_options_is_an_error() {
    let text = r#"
        [models]
        subwoofer = []
        logo = []

        [[entry]]
        path = "audio/in/interface"
        label = "Eingangs-Interface"
        label_en = "Input interface"
        group = "input"
        kind = "select"
        poll = "slow"
        class = "all"
    "#;
    match Catalog::parse(text) {
        Err(CatalogError::Invalid { path: invalid, .. }) => {
            assert_eq!(invalid, path("audio/in/interface"))
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn delay_must_be_whole_steps() {
    let catalog = Catalog::embedded();
    let delay = entry("audio/out/delay");
    assert_eq!(
        delay.validate(&json!(1.5), MONITOR, &catalog.classes),
        Err(ValueError::NotAStep)
    );
    assert_eq!(
        delay.validate(&json!(2.0), MONITOR, &catalog.classes),
        Ok(())
    );
}

/// The reason `Catalog::parse` gives for the one entry in `entry`, a TOML table body.
fn invalid_reason(entry: &str) -> Option<&'static str> {
    let text = format!("[models]\nsubwoofer = [\"KH 750\"]\nlogo = []\n\n[[entry]]\n{entry}");
    match Catalog::parse(&text) {
        Err(CatalogError::Invalid { reason, .. }) => Some(reason),
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => None,
    }
}

const SUBWOOFER_OUTPUT: &str = r#"
    path = "audio/out1/mute"
    label = "Stumm"
    label_en = "Mute"
    group = "output1"
    kind = "switch"
    poll = "fast"
    class = "subwoofer"
"#;

const LEVEL: &str = r#"
    path = "audio/out/level"
    label = "Pegel"
    label_en = "Level"
    group = "output"
    kind = "number"
    poll = "fast"
    class = "all"
"#;

#[test]
fn writable_on_a_product_outside_the_class_is_an_error() {
    assert!(invalid_reason(&format!("{SUBWOOFER_OUTPUT}writable_on = [\"KH 120 II\"]")).is_some());
    assert_eq!(
        invalid_reason(&format!("{SUBWOOFER_OUTPUT}writable_on = [\"KH 750\"]")),
        None
    );
}

#[test]
fn an_empty_number_range_is_an_error() {
    assert!(invalid_reason(&format!("{LEVEL}min = 5\nmax = 5")).is_some());
    assert!(invalid_reason(&format!("{LEVEL}min = 5\nmax = 10\nmax_subwoofer = 5")).is_some());
    assert_eq!(invalid_reason(&format!("{LEVEL}min = 0\nmax = 120")), None);
}

#[test]
fn every_entry_has_a_label_in_each_language() {
    for entry in &Catalog::embedded().entries {
        for language in Language::ALL {
            assert!(!entry.label(language).is_empty(), "{}", entry.path);
        }
    }
}

#[test]
fn eq_containers_per_model() {
    let catalog = Catalog::embedded();
    assert_eq!(catalog.eq_containers(MONITOR).len(), 2);
    assert_eq!(catalog.eq_containers(SUBWOOFER).len(), 12);
    let first = &catalog.eq_containers(MONITOR)[0];
    assert_eq!(first.path, path("audio/out/eq2"));
    assert_eq!(first.label(Language::German), "EQ 2");
    assert!(has(MONITOR, "audio/out/eq3/frequency"));
    assert!(!has(MONITOR, "audio/out1/eq2/gain"));
    assert!(entry("audio/out/eq2/gain").is_writable(SUBWOOFER));
    // Writes measured 2026-10-19 on every container of both models.
    assert!(entry("audio/out/eq3/gain").is_writable(MONITOR));
    assert!(entry("audio/out2/eq3/gain").is_writable(SUBWOOFER));
    assert!(entry("audio/out/eq1/in2/gain").is_writable(SUBWOOFER));
    assert!(entry("audio/out1/eq1/in1/gain").is_writable(SUBWOOFER));
    assert!(entry("audio/out2/eq1/in2/gain").is_writable(SUBWOOFER));
    assert!(!entry("audio/out/eq2/gain").is_writable(UNKNOWN_PRODUCT));
    assert!(entry("audio/out/eq2/gain").bands);
}

#[test]
fn a_band_write_needs_one_valid_entry() {
    let catalog = Catalog::embedded();
    let check = |text: &str, value| entry(text).validate(&value, MONITOR, &catalog.classes);
    assert_eq!(check("audio/out/eq2/gain", json!([null, 3.0])), Ok(()));
    assert_eq!(
        check("audio/out/eq2/gain", json!([null, null])),
        Err(ValueError::WrongType)
    );
    assert_eq!(
        check("audio/out/eq2/gain", json!([null, 30.0])),
        Err(ValueError::OutOfRange {
            min: -99.0,
            max: 24.0
        })
    );
    assert!(check("audio/out/eq2/type", json!([null, "FOO"])).is_err());
    assert_eq!(check("audio/out/eq2/type", json!(["NOTCH", null])), Ok(()));
    assert_eq!(check("audio/out/eq2/enabled", json!([true])), Ok(()));
    assert_eq!(
        check("audio/out/eq2/gain", json!(3.0)),
        Err(ValueError::WrongType)
    );
}

#[test]
fn only_the_kh750_eq1_containers_are_crossovers() {
    let catalog = Catalog::embedded();
    let crossovers: Vec<String> = catalog
        .eq
        .iter()
        .filter(|container| container.crossover)
        .map(|container| container.path.to_string())
        .collect();
    let expected: Vec<String> = ["out", "out1", "out2"]
        .iter()
        .flat_map(|output| ["in1", "in2"].map(|input| format!("audio/{output}/eq1/{input}")))
        .collect();
    assert_eq!(crossovers, expected);
}

#[test]
fn a_flat_full_array_is_a_valid_band_write() {
    let catalog = Catalog::embedded();
    let flat = json!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        entry("audio/out/eq2/boost").validate(&flat, MONITOR, &catalog.classes),
        Ok(())
    );
}

#[test]
fn the_d2_settings_are_in_the_catalog() {
    for output in ["out", "out1", "out2"] {
        let product = if output == "out" { MONITOR } else { SUBWOOFER };
        let persist = entry(&format!("audio/{output}/mute_persist"));
        assert_eq!(persist.kind, Kind::Switch);
        assert!(persist.is_writable(SUBWOOFER));
        assert_eq!(persist.is_writable(MONITOR), output == "out");
        let mixer = entry(&format!("audio/{output}/mixer/levels"));
        assert_eq!(mixer.kind, Kind::Number);
        assert!(mixer.is_writable(product));
        assert!(!mixer.is_writable(UNKNOWN_PRODUCT));
        assert_eq!(
            mixer.element_labels(Language::German),
            ["Eingang A", "Eingang B"]
        );
        assert_eq!(
            mixer.element_labels(Language::English),
            ["Input A", "Input B"]
        );
    }
    assert!(has(SUBWOOFER, "ui/auto_standby"));
    assert!(!has(MONITOR, "ui/auto_standby"));
    assert_eq!(entry("ui/auto_standby").kind, Kind::Display);
}

#[test]
fn a_list_value_is_written_whole() {
    let catalog = Catalog::embedded();
    let mixer = entry("audio/out/mixer/levels");
    let check = |value| mixer.validate(&value, MONITOR, &catalog.classes);
    assert_eq!(check(json!([0.0, -6.0])), Ok(()));
    assert_eq!(check(json!([null, -6.0])), Err(ValueError::WrongType));
    assert_eq!(check(json!([-6.0])), Err(ValueError::WrongType));
    assert_eq!(check(json!(-6.0)), Err(ValueError::WrongType));
    assert_eq!(
        check(json!([0.0, 3.0])),
        Err(ValueError::OutOfRange {
            min: -100.0,
            max: 0.0
        })
    );
}

#[test]
fn element_labels_must_match_in_both_languages() {
    let list = format!(
        "{LEVEL}min = -100\nmax = 0\nelements = [\"Eingang A\", \"Eingang B\"]\nelements_en = [\"Input A\"]"
    );
    assert!(invalid_reason(&list).is_some());
}

#[test]
fn a_read_only_kind_listing_writable_models_is_an_error() {
    let display = SUBWOOFER_OUTPUT.replace("kind = \"switch\"", "kind = \"display\"");
    assert!(invalid_reason(&format!("{display}writable_on = [\"KH 750\"]")).is_some());
    assert_eq!(invalid_reason(&display), None);
}

#[test]
fn invert_takes_a_shown_value_back_to_the_devices_unit() {
    for (conversion, device) in [
        (Conversion::KelvinToCelsius, 313.0),
        (Conversion::SamplesToMilliseconds, 96.0),
    ] {
        let shown = conversion.apply(device);
        assert!(
            (conversion.invert(shown) - device).abs() < 1e-9,
            "{conversion:?}"
        );
        assert_ne!(shown, device, "{conversion:?}");
    }
}

/// A level entry writable on the KH 750 with this `min` and `step`.
fn stepped(min: f64, step: f64) -> Catalog {
    let text = format!(
        "[models]\nsubwoofer = [\"KH 750\"]\nlogo = []\n\n[[entry]]\n{LEVEL}\
         min = {min:?}\nmax = 100.0\nstep = {step:?}\nwritable_on = [\"KH 750\"]"
    );
    Catalog::parse(&text).unwrap()
}

fn step_check(catalog: &Catalog, value: f64) -> Result<(), ValueError> {
    catalog.entries[0].validate(&json!(value), SUBWOOFER, &catalog.classes)
}

#[test]
fn the_step_grid_starts_at_the_minimum() {
    let catalog = stepped(0.5, 2.0);
    assert_eq!(step_check(&catalog, 2.5), Ok(()));
    assert_eq!(step_check(&catalog, 3.0), Err(ValueError::NotAStep));
    let catalog = stepped(0.0, 2.0);
    assert_eq!(step_check(&catalog, 3.0), Err(ValueError::NotAStep));
    let catalog = stepped(0.0, 0.5);
    assert_eq!(step_check(&catalog, 0.5), Ok(()));
}

#[test]
fn every_error_has_a_text() {
    let errors = [
        ValueError::NotAStep.to_string(),
        CatalogError::Invalid {
            path: path("audio/out/level"),
            reason: "select without options",
        }
        .to_string(),
        neumann_control_core::address::AddressError::Invalid.to_string(),
        neumann_control_core::ssc::client::SscError::Timeout.to_string(),
        neumann_control_core::ssc::wire::WireError::Malformed.to_string(),
    ];
    for text in errors {
        assert!(!text.is_empty());
    }
}
