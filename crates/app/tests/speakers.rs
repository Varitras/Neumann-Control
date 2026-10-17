//! Which cards the window shows: manual addresses, scans, one card per speaker.

mod support;

use egui_kittest::kittest::Queryable;
use neumann_control_app::texts;
use neumann_control_core::address::{AddressError, SSC_PORT};
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::{self, ManualSpeaker};
use neumann_control_core::simulator::{SimModel, Simulator};
use std::time::Duration;
use support::*;

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
fn scan_adds_found_speakers_once() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let found = Found {
        name: String::new(),
        product: Some("KH 750".to_owned()),
        serial: Some("SIM0007500".to_owned()),
        address: simulator.address(),
    };
    let mut harness = harness(path, vec![found]);
    harness.get_by_label(texts::SCAN).click();
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label("KH 750");
    harness.get_by_label(texts::MORE).click();
    harness.run_steps(2);
    harness.get_by_label(texts::SCAN).click();
    harness.run_steps(5);
    harness.get_by_label(&texts::speaker_count(1));
    // The card kept its state, so it was not recreated.
    harness.get_by_label(texts::LESS);
}

#[test]
fn scan_moves_a_known_speaker_to_its_new_address() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let stale = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: SSC_PORT,
        serial: Some(SIMULATED_SERIAL.to_owned()),
    };
    let path = settings_file(directory.path(), vec![stale]);
    let found = Found {
        name: "announced".to_owned(),
        product: None,
        serial: Some(SIMULATED_SERIAL.to_owned()),
        address: simulator.address(),
    };
    let mut harness = harness(path, vec![found]);
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label(texts::SCAN).click();
    wait_for_level(&mut harness, 100.0);
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn a_manual_speaker_found_at_a_second_address_stays_one_card() {
    let manual_simulator = Simulator::start(SimModel::Kh120Ii);
    // The manual card connects only after the scan created a card for the second address.
    manual_simulator.set_reply_delay(Duration::from_millis(300));
    let second_simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&manual_simulator)]);
    let found = Found {
        name: String::new(),
        product: Some("KH 120 II".to_owned()),
        serial: Some(SIMULATED_SERIAL.to_owned()),
        address: second_simulator.address(),
    };
    let mut harness = harness(path.clone(), vec![found]);
    harness.get_by_label(texts::SCAN).click();
    wait_until(&mut harness, "the serial in the manual entry", |_| {
        serials_remembered(&path, 1)
    });
    wait_until(&mut harness, "one card after the first scan", has_one_card);
    harness.get_by_label(texts::SCAN).click();
    harness.run_steps(5);
    wait_until(&mut harness, "one card after the second scan", has_one_card);
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
