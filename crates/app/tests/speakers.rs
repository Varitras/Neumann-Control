//! Which cards the window shows: addresses, speakers chosen from a scan, one card per speaker.

mod support;

use std::path::Path;
use std::time::Duration;

use egui_kittest::kittest::{NodeT, Queryable};
use neumann_control_app::texts;
use neumann_control_core::address::{parse_address, AddressError, SSC_PORT};
use neumann_control_core::discovery::Found;
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::{self, ManualSpeaker};
use neumann_control_core::simulator::{SimModel, Simulator};
use support::*;

const KH750_NAME: &str = "Sub";
const KH750_SERIAL: &str = "SIM0007500";
const KH120_NAME: &str = "Desk";

fn announced(simulator: &Simulator, name: &str, product: &str, serial: &str) -> Found {
    Found {
        name: name.to_owned(),
        product: Some(product.to_owned()),
        serial: Some(serial.to_owned()),
        address: simulator.address(),
    }
}

fn kh750_found(simulator: &Simulator) -> Found {
    announced(simulator, KH750_NAME, "KH 750", KH750_SERIAL)
}

fn kh120_found(simulator: &Simulator) -> Found {
    announced(simulator, KH120_NAME, "KH 120 II", SIMULATED_SERIAL)
}

fn saved_at(path: &Path, simulator: &Simulator) -> bool {
    settings::load(path)
        .manual
        .iter()
        .any(|entry| parse_address(&entry.address, entry.port) == Ok(simulator.address()))
}

#[test]
fn scan_shows_choices_without_adding_cards() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), vec![kh750_found(&simulator)]);
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, KH750_NAME);
    harness.get_by_label(texts::SCAN_CHOICE_TITLE);
    harness.get_by_label("KH 750");
    harness.get_by_label(&mask_host(&simulator.address()));
    harness.run_steps(5);
    harness.get_by_label(&texts::speaker_count(0));
    assert!(settings::load(&path).manual.is_empty());
    assert!(simulator.requests().is_empty());
}

#[test]
fn scan_says_when_nothing_was_found() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path, Vec::new());
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, texts::NOTHING_FOUND);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::SCAN_CHOICE_TITLE).is_none());
}

#[test]
fn a_failed_scan_says_so_in_the_dialog() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness_scanning(path, Err("no network".to_owned()));
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, texts::SCAN_FAILED);
    harness.get_by_label(texts::SCAN_CHOICE_TITLE);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::SCAN_CHOICE_TITLE).is_none());
    assert!(harness.query_by_label(texts::SCAN_FAILED).is_none());
}

#[test]
fn only_checked_speakers_get_cards() {
    let unchosen = Simulator::start(SimModel::Kh120Ii);
    let chosen = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let found = vec![kh120_found(&unchosen), kh750_found(&chosen)];
    let mut harness = harness(path.clone(), found);
    scan_and_add(&mut harness, &[KH750_NAME]);
    assert!(harness.query_by_label(texts::SCAN_CHOICE_TITLE).is_none());
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &chosen);
    harness.get_by_label(&texts::speaker_count(1));
    let saved = settings::load(&path).manual;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].serial.as_deref(), Some(KH750_SERIAL));
    assert!(saved_at(&path, &chosen));
    assert!(unchosen.requests().is_empty());
}

#[test]
fn cancel_adds_nothing() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), vec![kh750_found(&simulator)]);
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, KH750_NAME);
    harness.get_by_label(KH750_NAME).click();
    harness.run_steps(2);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(5);
    assert!(harness.query_by_label(texts::SCAN_CHOICE_TITLE).is_none());
    harness.get_by_label(&texts::speaker_count(0));
    assert!(settings::load(&path).manual.is_empty());
    assert!(simulator.requests().is_empty());
}

#[test]
fn known_speaker_is_marked_and_not_selectable() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut harness = harness(path.clone(), vec![kh120_found(&simulator)]);
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &simulator);
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(2);
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, KH120_NAME);
    harness.get_by_label(texts::ALREADY_ADDED);
    assert!(harness
        .get_by_label(KH120_NAME)
        .accesskit_node()
        .is_disabled());
    harness.get_by_label(KH120_NAME).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    harness.get_by_label(texts::SCAN_CHOICE_TITLE);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::speaker_count(1));
    assert_eq!(settings::load(&path).manual.len(), 1);
    // The card kept its state, so it was not recreated.
    harness.get_by_label(texts::LESS);
}

#[test]
fn chosen_speaker_survives_restart() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut first_run = harness(path.clone(), vec![kh750_found(&simulator)]);
    scan_and_add(&mut first_run, &[KH750_NAME]);
    wait_for_level(&mut first_run, 100.0);
    drop(first_run);
    let mut harness = harness(path.clone(), Vec::new());
    wait_for_level(&mut harness, 100.0);
    wait_for_initial_read(&mut harness, &simulator);
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label("KH 750");
    assert!(saved_at(&path, &simulator));
}

#[test]
fn rescan_updates_saved_address() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let stale = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: SSC_PORT,
        serial: Some(SIMULATED_SERIAL.to_owned()),
    };
    let path = settings_file(directory.path(), vec![stale]);
    let mut harness = harness(path.clone(), vec![kh120_found(&simulator)]);
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, KH120_NAME);
    harness.get_by_label(texts::ALREADY_ADDED);
    assert!(saved_at(&path, &simulator));
    assert_eq!(settings::load(&path).manual.len(), 1);
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn add_address_refuses_ipv4_with_hint() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text("192.168.1.20");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::describe_address_error(
        &AddressError::Ipv4NotSupported,
    ));
    assert!(settings::load(&path).manual.is_empty());
    harness.get_by_label(&texts::speaker_count(0));
}

#[test]
fn add_address_saves_the_speaker_and_shows_its_card() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text(LOOPBACK);
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::ADD_ADDRESS_TITLE).is_none());
    let saved = settings::load(&path).manual;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].address, LOOPBACK);
    assert_eq!(saved[0].port, SSC_PORT);
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn a_manual_speaker_found_at_a_second_address_stays_one_card() {
    let manual_simulator = Simulator::start(SimModel::Kh120Ii);
    // The manual card connects only after the chosen card for the second address started.
    manual_simulator.set_reply_delay(Duration::from_millis(300));
    let second_simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&manual_simulator)]);
    let mut harness = harness(path.clone(), vec![kh120_found(&second_simulator)]);
    scan_and_add(&mut harness, &[KH120_NAME]);
    wait_until(&mut harness, "the serial in both entries", |_| {
        serials_remembered(&path, 2)
    });
    wait_until(&mut harness, "one card after the first scan", has_one_card);
    harness.get_by_label(texts::SCAN).click();
    wait_for_label(&mut harness, texts::ALREADY_ADDED);
    harness.get_by_label(texts::CANCEL).click();
    harness.run_steps(5);
    wait_until(&mut harness, "one card after the second scan", has_one_card);
    assert!(serials_remembered(&path, 1));
}

#[test]
fn two_manual_entries_for_one_speaker_stay_one_card() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let second = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(
        directory.path(),
        vec![manual_for(&first), manual_for(&second)],
    );
    let mut harness = harness(path.clone(), Vec::new());
    wait_until(&mut harness, "both serials remembered", |_| {
        serials_remembered(&path, 2)
    });
    wait_until(&mut harness, "one card", has_one_card);
}

#[test]
fn add_address_names_an_address_already_listed() {
    let directory = tempfile::tempdir().unwrap();
    let listed = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: SSC_PORT,
        serial: None,
    };
    let path = settings_file(directory.path(), vec![listed]);
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD)
        .type_text("[::1]");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_ALREADY_LISTED);
    harness.get_by_label(texts::ADD_ADDRESS_TITLE);
    assert_eq!(settings::load(&path).manual.len(), 1);
}
