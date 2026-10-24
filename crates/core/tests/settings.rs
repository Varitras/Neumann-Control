use neumann_control_core::settings::{
    load, load_checked, save, BandCurves, CompactButton, CrossoverGuard, DelayUnit, DeskSettings,
    EqPreset, GroupSettings, Language, LanguageChoice, Linkable, LoadOutcome, ManualSpeaker,
    MeterFloor, Settings, SettingsPreset, StoredStripLayout, StripElement, SystemAudioSettings,
    ThemeChoice, WindowGeometry, SETTINGS_FILE,
};
use neumann_control_core::ssc::wire::SscPath;
use std::fs;

fn sample() -> Settings {
    Settings {
        manual: vec![
            ManualSpeaker {
                address: "fe80::2a36:38ff:fe12:3456".to_string(),
                port: 45,
                serial: Some("SIM0001234".to_string()),
            },
            ManualSpeaker {
                address: "192.0.2.10".to_string(),
                port: 45,
                serial: None,
            },
        ],
        window: Some(WindowGeometry {
            x: 10.0,
            y: 20.0,
            width: 800.0,
            height: 600.0,
        }),
        desk: DeskSettings::default(),
        ..Settings::default()
    }
}

#[test]
fn missing_file_gives_default() {
    let directory = tempfile::tempdir().unwrap();
    let loaded = load(&directory.path().join(SETTINGS_FILE));
    assert_eq!(loaded, Settings::default());
}

#[test]
fn round_trip_keeps_values() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    save(&sample(), &path).unwrap();
    assert_eq!(load(&path), sample());
    assert!(!directory.path().join("settings.json.tmp").exists());
}

#[test]
fn save_replaces_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    save(&Settings::default(), &path).unwrap();
    save(&sample(), &path).unwrap();
    assert_eq!(load(&path), sample());
}

#[test]
fn malformed_file_is_moved_aside() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, "{ not json").unwrap();
    assert_eq!(load(&path), Settings::default());
    assert!(!path.exists());
    assert_eq!(
        fs::read_to_string(directory.path().join("settings.json.bad")).unwrap(),
        "{ not json"
    );
}

#[test]
fn existing_bad_file_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(directory.path().join("settings.json.bad"), "first").unwrap();
    fs::write(&path, "second").unwrap();
    assert_eq!(load(&path), Settings::default());
    assert_eq!(
        fs::read_to_string(directory.path().join("settings.json.bad")).unwrap(),
        "first"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("settings.json.bad.1")).unwrap(),
        "second"
    );
}

#[test]
fn wrong_type_is_malformed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual": "oops"}"#).unwrap();
    assert_eq!(load(&path), Settings::default());
    assert!(directory.path().join("settings.json.bad").exists());
}

#[test]
fn unknown_and_missing_fields_are_tolerated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(
        &path,
        r#"{"future": 1, "manual": [{"address": "192.0.2.10", "port": 45, "extra": true}]}"#,
    )
    .unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.manual.len(), 1);
    assert_eq!(loaded.manual[0].serial, None);
    assert_eq!(loaded.window, None);
    assert!(path.exists());
}

/// Values of a newer version would be dropped by the next save: a copy keeps them, as for
/// values that are not understood.
#[test]
fn values_of_a_newer_version_keep_a_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text = r#"{"future": {"feature": 1}, "desk": {"compact": true, "newer": "x"}}"#;
    fs::write(&path, text).unwrap();
    let loaded = load_checked(&path);
    assert_eq!(loaded.outcome, LoadOutcome::PartlyUnderstood);
    assert!(loaded.settings.desk.compact);
    let copy = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert_eq!(fs::read_to_string(copy).unwrap(), text);
}

#[test]
fn a_file_of_this_version_is_read_without_a_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    save(&sample(), &path).unwrap();
    assert_eq!(load_checked(&path).outcome, LoadOutcome::Read);
    assert!(!directory
        .path()
        .join(format!("{SETTINGS_FILE}.bad"))
        .exists());
}

/// The desk's look is not worth the remembered speakers.
#[test]
fn a_desk_of_the_wrong_type_keeps_the_speakers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text = r#"{"desk": 3, "manual": [{"address": "192.0.2.10", "port": 45}]}"#;
    fs::write(&path, text).unwrap();
    let loaded = load_checked(&path);
    assert_eq!(loaded.outcome, LoadOutcome::PartlyUnderstood);
    assert_eq!(loaded.settings.manual.len(), 1);
    assert_eq!(loaded.settings.desk, DeskSettings::default());
}

/// A file that can be neither read nor moved aside (held open by another program) must not
/// be written over with the defaults.
#[cfg(windows)]
#[test]
fn a_file_that_cannot_be_read_or_moved_is_untouchable() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    save(&sample(), &path).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert_eq!(load_checked(&path).outcome, LoadOutcome::Untouchable);
    drop(held);
    assert_eq!(load(&path), sample());
}

#[test]
fn old_settings_file_loads_with_desk_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[],"window":null}"#).unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.desk, DeskSettings::default());
    assert_eq!(loaded.desk.theme, ThemeChoice::System);
    assert_eq!(loaded.desk.meter_floor.db(), -120.0);
    assert!(loaded.desk.show_response_times);
    assert_eq!(loaded.desk.language, LanguageChoice::System);
    assert!(!loaded.desk.compact);
    assert_eq!(loaded.compact_window, None);
}

#[test]
fn desk_settings_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let settings = Settings {
        desk: DeskSettings {
            theme: ThemeChoice::Dark,
            meter_floor: MeterFloor::Minus60,
            show_response_times: false,
            language: LanguageChoice::English,
            compact: true,
            crossovers: CrossoverGuard::Open,
            ..DeskSettings::default()
        },
        compact_window: Some(WindowGeometry {
            x: 10.0,
            y: 20.0,
            width: 400.0,
            height: 300.0,
        }),
        ..Settings::default()
    };
    save(&settings, &path).unwrap();
    assert_eq!(load(&path), settings);
}

#[test]
fn old_settings_file_loads_without_groups() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[],"window":null}"#).unwrap();
    let loaded = load(&path);
    assert!(loaded.groups.is_empty());
    assert!(loaded.order.is_empty());
}

#[test]
fn groups_and_order_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let settings = Settings {
        groups: vec![GroupSettings {
            name: "Front".to_owned(),
            color: 1,
            members: vec!["serial:SIM0001234".to_owned()],
            linked: vec![Linkable::Level, Linkable::Mute],
        }],
        order: vec![
            "serial:SIM0001234".to_owned(),
            "serial:SIM0007500".to_owned(),
        ],
        ..Settings::default()
    };
    save(&settings, &path).unwrap();
    assert_eq!(load(&path), settings);
}

#[test]
fn each_mode_keeps_its_own_window() {
    let desk = WindowGeometry {
        x: 0.0,
        y: 0.0,
        width: 900.0,
        height: 600.0,
    };
    let compact = WindowGeometry {
        width: 480.0,
        height: 360.0,
        ..desk
    };
    let mut settings = Settings::default();
    assert_eq!(
        settings.switch_mode(Some(desk)),
        None,
        "compact never had a window"
    );
    assert!(settings.desk.compact);
    assert_eq!(settings.switch_mode(Some(compact)), Some(desk));
    assert!(!settings.desk.compact);
    assert_eq!(settings.switch_mode(None), Some(compact));
    assert_eq!(settings.window_of_mode(), Some(compact));
}

#[test]
fn a_value_this_version_does_not_know_costs_only_that_value() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    // As a later version or a hand edit might leave it: an unknown language and meter floor,
    // a group without a colour, a speaker entry without a port.
    let text = r#"{
        "manual": [{"address": "fe80::1%7", "port": 45, "serial": "SIM1"}, {"address": "fe80::2%7"}],
        "desk": {"language": "French", "meter_floor": "Minus200", "show_response_times": false,
                 "delay_unit": "Furlongs"},
        "groups": [
            {"name": "Front", "members": ["serial:SIM1"], "linked": ["Level", "Delay"]},
            {"name": "Sub", "color": 1, "members": []}
        ],
        "order": ["serial:SIM1"]
    }"#;
    fs::write(&path, text).unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.manual.len(), 1, "the readable speaker stays");
    assert_eq!(loaded.desk.language, LanguageChoice::System);
    assert_eq!(loaded.desk.meter_floor, MeterFloor::Minus120);
    assert_eq!(loaded.desk.delay_unit, DelayUnit::Milliseconds);
    assert!(
        !loaded.desk.show_response_times,
        "a readable neighbour stays"
    );
    // A group keeps its members when its colour is missing or a link is unknown.
    assert_eq!(loaded.groups.len(), 2);
    assert_eq!(loaded.groups[0].members, vec!["serial:SIM1".to_owned()]);
    assert_eq!(loaded.groups[0].linked, vec![Linkable::Level]);
    assert_eq!(loaded.groups[1].color, 1);
    assert_eq!(loaded.order, vec!["serial:SIM1".to_owned()]);
    // The original stays readable for whoever wants the dropped values back.
    let copy = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert_eq!(fs::read_to_string(copy).unwrap(), text);
    assert!(path.exists());
}

#[test]
fn an_unreadable_file_is_moved_aside_not_taken_for_missing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    // A folder in its place cannot be read, and is not "not found" either.
    fs::create_dir(&path).unwrap();
    assert_eq!(load(&path), Settings::default());
    assert!(!path.exists());
    let set_aside = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert!(set_aside.is_dir());
}

#[test]
fn an_unknown_list_entry_alone_keeps_a_copy_of_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text = r#"{"manual": [{"address": "fe80::2%7"}]}"#;
    fs::write(&path, text).unwrap();
    assert!(load(&path).manual.is_empty());
    let copy = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert_eq!(fs::read_to_string(copy).unwrap(), text);
}

#[test]
fn a_single_unknown_value_keeps_a_copy_of_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text = r#"{"desk": {"language": "French"}}"#;
    fs::write(&path, text).unwrap();
    assert_eq!(load(&path).desk.language, LanguageChoice::System);
    let copy = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert_eq!(fs::read_to_string(copy).unwrap(), text);
}

#[test]
fn a_file_of_the_previous_version_keeps_its_speakers_and_window() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    // Before groups, order, language and the compact mode existed.
    let text = r#"{
        "manual": [{"address": "fe80::1%7", "port": 45, "serial": "SIM1"}],
        "window": {"x": 10.0, "y": 20.0, "width": 900.0, "height": 600.0},
        "desk": {"theme": "Dark", "meter_floor": "Minus90", "show_response_times": false}
    }"#;
    fs::write(&path, text).unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.manual[0].serial.as_deref(), Some("SIM1"));
    assert_eq!(
        loaded.window_of_mode().map(|window| window.width),
        Some(900.0)
    );
    assert_eq!(loaded.desk.theme, ThemeChoice::Dark);
    assert_eq!(loaded.desk.meter_floor, MeterFloor::Minus90);
    assert!(!directory
        .path()
        .join(format!("{SETTINGS_FILE}.bad"))
        .exists());
}

#[test]
fn a_stored_language_keeps_its_choice() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[],"desk":{"language":"German"}}"#).unwrap();
    assert_eq!(load(&path).desk.language, LanguageChoice::German);
}

#[test]
fn like_windows_follows_the_system_language() {
    assert_eq!(
        LanguageChoice::System.resolve(Language::English),
        Language::English
    );
    assert_eq!(
        LanguageChoice::System.resolve(Language::German),
        Language::German
    );
    assert_eq!(
        LanguageChoice::German.resolve(Language::English),
        Language::German
    );
    assert_eq!(
        LanguageChoice::English.resolve(Language::German),
        Language::English
    );
}

#[test]
fn crossovers_stay_protected_unless_opened_on_purpose() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[],"desk":{}}"#).unwrap();
    assert_eq!(load(&path).desk.crossovers, CrossoverGuard::Protected);
    fs::write(&path, r#"{"manual":[],"desk":{"crossovers":"maybe"}}"#).unwrap();
    assert_eq!(load(&path).desk.crossovers, CrossoverGuard::Protected);
    fs::write(&path, r#"{"manual":[],"desk":{"crossovers":"Open"}}"#).unwrap();
    assert_eq!(load(&path).desk.crossovers, CrossoverGuard::Open);
}

#[test]
fn presets_round_trip_and_a_broken_one_is_left_out() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let settings = Settings {
        presets: vec![SettingsPreset {
            name: "Night".to_owned(),
            product: "KH 120 II".to_owned(),
            values: [("audio/out/level".to_owned(), serde_json::json!(80.0))].into(),
        }],
        eq_presets: vec![EqPreset {
            name: "Flat-ish".to_owned(),
            bands: 2,
            leaves: [("gain".to_owned(), serde_json::json!([0.0, 1.0]))].into(),
        }],
        ..Settings::default()
    };
    save(&settings, &path).unwrap();
    assert_eq!(load(&path), settings);
    fs::write(
        &path,
        r#"{"manual":[],"presets":[{"name":"ok","product":"KH 750","values":{}},{"name":3}],"eq_presets":"no"}"#,
    )
    .unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.presets.len(), 1);
    assert!(loaded.eq_presets.is_empty());
}

#[test]
fn the_windows_volume_strip_is_off_unless_chosen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[]}"#).unwrap();
    assert_eq!(load(&path).system_audio, SystemAudioSettings::default());
    assert!(!load(&path).system_audio.shown);
    fs::write(
        &path,
        r#"{"manual":[],"system_audio":{"shown":true,"device":"{0.0.0.00000000}.{id}"}}"#,
    )
    .unwrap();
    let loaded = load(&path).system_audio;
    assert!(loaded.shown);
    assert_eq!(loaded.device.as_deref(), Some("{0.0.0.00000000}.{id}"));
    fs::write(&path, r#"{"manual":[],"system_audio":7}"#).unwrap();
    assert_eq!(load(&path).system_audio, SystemAudioSettings::default());
}

#[test]
fn the_eq_display_keeps_its_choices_and_falls_back_on_nonsense() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"manual":[]}"#).unwrap();
    let display = load(&path).eq_display;
    assert_eq!(display.band_curves, BandCurves::All);
    assert!(display.band_colors.is_empty() && display.curve_colors.is_empty());
    fs::write(
        &path,
        r##"{"manual":[],"eq_display":{"band_curves":"ChosenAndHovered","band_colors":{"3":"#ff0000"},"curve_colors":{"eq2":"#00ff00"}}}"##,
    )
    .unwrap();
    let display = load(&path).eq_display;
    assert_eq!(display.band_curves, BandCurves::ChosenAndHovered);
    assert_eq!(
        display.band_colors.get("3").map(String::as_str),
        Some("#ff0000")
    );
    assert_eq!(
        display.curve_colors.get("eq2").map(String::as_str),
        Some("#00ff00")
    );
    fs::write(
        &path,
        r#"{"manual":[],"eq_display":{"band_curves":"sometimes"}}"#,
    )
    .unwrap();
    assert_eq!(load(&path).eq_display.band_curves, BandCurves::All);
}

/// A file from before the setting shows the master, as the app did then.
#[test]
fn an_older_file_shows_the_master() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"desk": {"show_response_times": false}}"#).unwrap();
    assert!(load(&path).desk.show_master);
}

/// The lowest level a meter shows, in 10 dB steps from -120 to -30 dB.
#[test]
fn the_meter_floors_go_in_10_db_steps() {
    let floors: Vec<f64> = MeterFloor::ALL.iter().map(|floor| floor.db()).collect();
    let steps: Vec<f64> = (3..=12).rev().map(|tens| -10.0 * f64::from(tens)).collect();
    assert_eq!(floors, steps);
}

/// Files written with the three floors before keep them.
#[test]
fn earlier_meter_floors_still_load() {
    for (name, db) in [("Minus120", -120.0), ("Minus90", -90.0), ("Minus60", -60.0)] {
        let floor: MeterFloor = serde_json::from_str(&format!("\"{name}\"")).unwrap();
        assert_eq!(floor.db(), db);
    }
}

fn element(text: &str) -> StripElement {
    StripElement::Path(SscPath::parse(text).unwrap())
}

#[test]
fn strip_layouts_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let mut settings = Settings::default();
    let layout = StoredStripLayout {
        above: vec![element("audio/out/phaseinversion")],
        below: vec![element("audio/out/mute"), StripElement::Eq],
    };
    settings
        .desk
        .strip_layouts
        .insert("KH 120 II".to_owned(), layout);
    save(&settings, &path).unwrap();
    assert_eq!(load(&path), settings);
}

#[test]
fn a_file_without_strip_layouts_loads_none() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    fs::write(&path, r#"{"desk": {"compact": false}}"#).unwrap();
    assert!(load(&path).desk.strip_layouts.is_empty());
    assert!(!directory
        .path()
        .join(format!("{SETTINGS_FILE}.bad"))
        .exists());
}

#[test]
fn an_unreadable_strip_element_is_left_out_and_keeps_a_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text = r#"{"desk": {"strip_layouts": {"KH 120 II": {"above": ["audio//out", "audio/out/delay"], "below": []}}}}"#;
    fs::write(&path, text).unwrap();
    let layouts = load(&path).desk.strip_layouts;
    assert_eq!(layouts["KH 120 II"].above, vec![element("audio/out/delay")]);
    let copy = directory.path().join(format!("{SETTINGS_FILE}.bad"));
    assert_eq!(fs::read_to_string(copy).unwrap(), text);
}

#[test]
fn compact_layouts_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let mut settings = Settings::default();
    let switch = CompactButton::Switch(SscPath::parse("device/identification/visual").unwrap());
    let buttons = vec![CompactButton::Details, switch, CompactButton::Eq];
    settings
        .desk
        .compact_layouts
        .insert("KH 750".to_owned(), buttons);
    save(&settings, &path).unwrap();
    assert_eq!(load(&path), settings);
}

#[test]
fn an_unreadable_compact_button_is_left_out_alone() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(SETTINGS_FILE);
    let text =
        r#"{"desk": {"compact_layouts": {"KH 750": ["audio//out", "details"], "KH 80": 3}}}"#;
    fs::write(&path, text).unwrap();
    let layouts = load(&path).desk.compact_layouts;
    assert_eq!(layouts["KH 750"], vec![CompactButton::Details]);
    assert!(!layouts.contains_key("KH 80"));
}
