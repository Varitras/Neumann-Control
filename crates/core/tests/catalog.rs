use neumann_control_core::catalog::{Catalog, CatalogError, Conversion, Entry, Kind, ValueError};
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
    for candidate in &Catalog::embedded().entries {
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
        group = "output"
        kind = "switch"
        poll = "fast"
        class = "all"

        [[entry]]
        path = "audio/out/mute"
        label = "Stumm"
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
