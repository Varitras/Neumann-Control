//! Which speakers the window shows: addresses, speakers chosen from a scan, each once.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::support::*;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::rediscovery::UNREACHABLE_GRACE_SECONDS;
use neumann_control_app::texts;
use neumann_control_core::address::{parse_address, AddressError, SSC_PORT};
use neumann_control_core::discovery::Found;
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::{self, ManualSpeaker, SETTINGS_FILE};
use neumann_control_core::simulator::{Fault, SimModel, Simulator};

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

const SCROLL_FRAMES: usize = 4;
/// Longer than the test meter interval (100 ms), so a poll is waiting for its reply.
const SILENCE_SETTLE: Duration = Duration::from_millis(300);
/// Far below the 3 s a joined worker could take, far above two frames.
const STALL_LIMIT: Duration = Duration::from_secs(1);
/// Below the client's 3 s reply timeout, so the speaker stays connecting instead of failing.
const SLOW_CONNECT_REPLY: Duration = Duration::from_millis(1500);

/// The unreachable status shows a few frames after the outage began, and the scan answers a few
/// frames after it started.
const GRACE_MARGIN_SECONDS: f64 = 2.0;

fn app_time(harness: &Harness<'static, NeumannApp>) -> f64 {
    harness.ctx.input(|input| input.time)
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
fn scan_shows_choices_without_adding_speakers() {
    let simulator = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), vec![kh750_found(&simulator)]);
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, KH750_NAME);
    harness.get_by_label(texts::SCAN_CHOICE_TITLE.get());
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
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, texts::NOTHING_FOUND.get());
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label(texts::SCAN_CHOICE_TITLE.get())
        .is_none());
}

#[test]
fn a_failed_scan_says_so_in_the_dialog() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness_scanning(path, Err("no network".to_owned()));
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, texts::SCAN_FAILED.get());
    harness.get_by_label(texts::SCAN_CHOICE_TITLE.get());
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label(texts::SCAN_CHOICE_TITLE.get())
        .is_none());
    assert!(harness.query_by_label(texts::SCAN_FAILED.get()).is_none());
}

#[test]
fn only_checked_speakers_are_added() {
    let unchosen = Simulator::start(SimModel::Kh120Ii);
    let chosen = Simulator::start(SimModel::Kh750);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let found = vec![kh120_found(&unchosen), kh750_found(&chosen)];
    let mut harness = harness(path.clone(), found);
    scan_and_add(&mut harness, &[KH750_NAME]);
    assert!(harness
        .query_by_label(texts::SCAN_CHOICE_TITLE.get())
        .is_none());
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
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, KH750_NAME);
    harness.get_by_label(KH750_NAME).click();
    harness.run_steps(2);
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(5);
    assert!(harness
        .query_by_label(texts::SCAN_CHOICE_TITLE.get())
        .is_none());
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
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, KH120_NAME);
    harness.get_by_label(texts::ALREADY_ADDED.get());
    assert!(harness
        .get_by_label(KH120_NAME)
        .accesskit_node()
        .is_disabled());
    harness.get_by_label(KH120_NAME).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADD.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::SCAN_CHOICE_TITLE.get());
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::speaker_count(1));
    assert_eq!(settings::load(&path).manual.len(), 1);
    // The strip kept its values, so it was not recreated.
    harness.get_by_label(&readout(100.0));
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
    harness.get_by_label(KH750_DEVICE_NAME);
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
    harness.get_by_label(texts::SCAN.get()).click();
    // Not the name: the unconnected strip shows the announced name as well.
    wait_for_label(&mut harness, texts::ALREADY_ADDED.get());
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
    harness.get_by_label(texts::ADD_ADDRESS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD.get()).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD.get())
        .type_text("192.168.1.20");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD.get()).click();
    harness.run_steps(2);
    harness.get_by_label(&texts::describe_address_error(
        &AddressError::Ipv4NotSupported,
    ));
    assert!(settings::load(&path).manual.is_empty());
    harness.get_by_label(&texts::speaker_count(0));
}

#[test]
fn add_address_saves_the_speaker_and_shows_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path.clone(), Vec::new());
    harness.get_by_label(texts::ADD_ADDRESS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD.get()).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD.get())
        .type_text(LOOPBACK);
    harness.run_steps(1);
    harness.get_by_label(texts::ADD.get()).click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label(texts::ADD_ADDRESS_TITLE.get())
        .is_none());
    let saved = settings::load(&path).manual;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].address, LOOPBACK);
    assert_eq!(saved[0].port, SSC_PORT);
    harness.get_by_label(&texts::speaker_count(1));
}

#[test]
fn a_manual_speaker_found_at_a_second_address_is_shown_once() {
    let manual_simulator = Simulator::start(SimModel::Kh120Ii);
    // The manual entry connects only after the chosen one for the second address started.
    manual_simulator.set_reply_delay(Duration::from_millis(300));
    let second_simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&manual_simulator)]);
    let mut harness = harness(path.clone(), vec![kh120_found(&second_simulator)]);
    scan_and_add(&mut harness, &[KH120_NAME]);
    wait_until(&mut harness, "the serial in both entries", |_| {
        serials_remembered(&path, 2)
    });
    wait_until(
        &mut harness,
        "one speaker after the first scan",
        shows_one_speaker,
    );
    harness.get_by_label(texts::SCAN.get()).click();
    wait_for_label(&mut harness, texts::ALREADY_ADDED.get());
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(5);
    wait_until(
        &mut harness,
        "one speaker after the second scan",
        shows_one_speaker,
    );
    assert!(serials_remembered(&path, 1));
}

#[test]
fn two_manual_entries_for_one_speaker_are_shown_once() {
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
    wait_until(&mut harness, "one speaker", shows_one_speaker);
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
    harness.get_by_label(texts::ADD_ADDRESS.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_FIELD.get()).click();
    harness.run_steps(1);
    harness
        .get_by_label(texts::ADDRESS_FIELD.get())
        .type_text("[::1]");
    harness.run_steps(1);
    harness.get_by_label(texts::ADD.get()).click();
    harness.run_steps(2);
    harness.get_by_label(texts::ADDRESS_ALREADY_LISTED.get());
    harness.get_by_label(texts::ADD_ADDRESS_TITLE.get());
    assert_eq!(settings::load(&path).manual.len(), 1);
}

/// Opens the device panel of the only speaker and asks to remove it.
fn ask_to_remove(harness: &mut Harness<'static, NeumannApp>) {
    open_panel(harness, KH120_DEVICE_NAME);
    harness.get_by_label(texts::REMOVE.get()).click();
    wait_for_label(harness, texts::REMOVE_TITLE.get());
}

#[test]
fn remove_needs_confirmation_and_forgets_speaker() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let path = directory.path().join(SETTINGS_FILE);
    ask_to_remove(&mut harness);
    harness.get_by_label(&texts::speaker_count(1));
    assert_eq!(settings::load(&path).manual.len(), 1);
    let confirm = harness
        .query_all_by_label(texts::REMOVE.get())
        .last()
        .expect("the dialog's button");
    confirm.click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::REMOVE_TITLE.get()).is_none());
    harness.get_by_label(&texts::speaker_count(0));
    assert!(settings::load(&path).manual.is_empty());
    drop(harness);
    let mut restarted = crate::support::harness(path, Vec::new());
    restarted.run_steps(2);
    restarted.get_by_label(&texts::speaker_count(0));
}

#[test]
fn cancel_remove_keeps_speaker() {
    let (_simulator, directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    let path = directory.path().join(SETTINGS_FILE);
    ask_to_remove(&mut harness);
    harness.get_by_label(texts::CANCEL.get()).click();
    harness.run_steps(2);
    assert!(harness.query_by_label(texts::REMOVE_TITLE.get()).is_none());
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label(&readout(100.0));
    assert_eq!(settings::load(&path).manual.len(), 1);
}

#[test]
fn a_moved_speaker_is_found_at_the_start() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let stale = ManualSpeaker {
        address: LOOPBACK.to_owned(),
        port: SSC_PORT,
        serial: Some(SIMULATED_SERIAL.to_owned()),
    };
    let path = settings_file(directory.path(), vec![stale]);
    let mut harness = harness(path.clone(), vec![kh120_found(&simulator)]);
    wait_for_level(&mut harness, 100.0);
    assert!(saved_at(&path, &simulator));
    assert!(
        harness.query_by_label(texts::ALREADY_ADDED.get()).is_none(),
        "no dialog"
    );
}

#[test]
fn a_speaker_that_stays_unreachable_is_looked_for_again() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let directory = tempfile::tempdir().unwrap();
    let mut entry = manual_for(&first);
    entry.serial = Some(SIMULATED_SERIAL.to_owned());
    let path = settings_file(directory.path(), vec![entry]);
    let shared: SharedScan = std::sync::Arc::new(std::sync::Mutex::new(Ok(Vec::new())));
    let mut harness = harness_with_shared_scan(path.clone(), shared.clone());
    wait_for_level(&mut harness, 100.0);
    first.stop();
    wait_for_label(&mut harness, texts::UNREACHABLE.get());
    let since = app_time(&harness);
    let moved = Simulator::start(SimModel::Kh120Ii);
    *shared.lock().unwrap() = Ok(vec![kh120_found(&moved)]);
    // Not looked for during the grace, and right after it.
    while app_time(&harness) < since + UNREACHABLE_GRACE_SECONDS - GRACE_MARGIN_SECONDS {
        harness.step();
        assert!(!saved_at(&path, &moved), "looked for during the grace");
    }
    while !saved_at(&path, &moved) {
        harness.step();
        std::thread::sleep(POLL_PAUSE);
        assert!(
            app_time(&harness) < since + UNREACHABLE_GRACE_SECONDS + GRACE_MARGIN_SECONDS,
            "not looked for after the grace"
        );
    }
    wait_until(&mut harness, "the reconnect", |harness| {
        harness.query_by_label(texts::UNREACHABLE.get()).is_none()
    });
}

/// A speaker that connects later gets its key from its serial; an open panel of another speaker
/// stays with that one.
#[test]
fn an_open_panel_stays_when_another_speaker_learns_its_serial() {
    let first = Simulator::start(SimModel::Kh120Ii);
    let reserved = Simulator::start(SimModel::Kh750);
    let late_address = reserved.address();
    let late_entry = manual_for(&reserved);
    reserved.stop();
    let directory = tempfile::tempdir().unwrap();
    let mut entry = manual_for(&first);
    entry.serial = Some(SIMULATED_SERIAL.to_owned());
    let path = settings_file(directory.path(), vec![entry, late_entry]);
    let mut harness = harness(path, Vec::new());
    wait_for_label(&mut harness, KH120_DEVICE_NAME);
    open_panel(&mut harness, KH120_DEVICE_NAME);
    let _late = Simulator::start_on(SimModel::Kh750, late_address).unwrap();
    wait_for_label(&mut harness, KH750_DEVICE_NAME);
    harness.run_steps(2);
    // The strip and the panel each name the speaker the panel belongs to.
    let named = |name: &str| harness.query_all_by_label(name).count();
    assert_eq!(named(KH120_DEVICE_NAME), 2, "the panel left its speaker");
    assert_eq!(named(KH750_DEVICE_NAME), 1);
}

/// Asks to remove the speaker named `name` and confirms.
fn remove_named(harness: &mut Harness<'static, NeumannApp>, name: &str) {
    open_panel(harness, name);
    ask_in_open_panel(harness);
    confirm_remove(harness);
}

/// The KH 750's panel is taller than the test window; "Remove" sits at its foot.
fn ask_in_open_panel(harness: &mut Harness<'static, NeumannApp>) {
    harness.get_by_label(texts::REMOVE.get()).scroll_to_me();
    // Scrolling is animated.
    harness.run_steps(SCROLL_FRAMES);
    harness.get_by_label(texts::REMOVE.get()).click();
    wait_for_label(harness, texts::REMOVE_TITLE.get());
}

fn confirm_remove(harness: &mut Harness<'static, NeumannApp>) {
    harness
        .query_all_by_label(texts::REMOVE.get())
        .last()
        .expect("the dialog's button")
        .click();
    harness.run_steps(2);
}

#[test]
fn removing_the_second_speaker_keeps_the_first() {
    let (_simulators, _directory, mut harness) =
        desk_of(&[SimModel::Kh120Ii, SimModel::Kh750], BTreeMap::new());
    remove_named(&mut harness, KH750_DEVICE_NAME);
    harness.get_by_label(&texts::speaker_count(1));
    harness.get_by_label(KH120_DEVICE_NAME);
    assert!(harness.query_by_label(KH750_DEVICE_NAME).is_none());
}

/// The confirmation stays open while the speaker connects and its key becomes its serial; the
/// speaker is removed all the same.
#[test]
fn a_speaker_that_connects_while_asked_about_is_removed() {
    let simulator = Simulator::start(SimModel::Kh750);
    // Slow replies keep it connecting (no model, no serial yet) while the question is asked.
    simulator.set_reply_delay(SLOW_CONNECT_REPLY);
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), vec![manual_for(&simulator)]);
    let mut harness = harness(path, Vec::new());
    harness.set_size(EXPANDED_WINDOW_SIZE.into());
    harness.run_steps(2);
    harness
        .get_by_label(&mask_host(&simulator.address()))
        .click();
    wait_for_label(&mut harness, texts::REMOVE.get());
    ask_in_open_panel(&mut harness);
    simulator.set_reply_delay(REPLY_DELAY);
    wait_for_label(&mut harness, KH750_DEVICE_NAME);
    confirm_remove(&mut harness);
    harness.get_by_label(&texts::speaker_count(0));
}

/// A worker can wait up to the client's 3 s reply timeout before it sees the stop; removing its
/// speaker must not make the window wait with it.
#[test]
fn removing_a_speaker_that_does_not_answer_does_not_stall_the_window() {
    let (simulator, _directory, mut harness) = connected_speaker(SimModel::Kh120Ii);
    ask_to_remove(&mut harness);
    simulator.inject(Fault::Silence);
    // The next meter poll is sent and stays unanswered.
    std::thread::sleep(SILENCE_SETTLE);
    let started = Instant::now();
    confirm_remove(&mut harness);
    let took = started.elapsed();
    harness.get_by_label(&texts::speaker_count(0));
    assert!(took < STALL_LIMIT, "the window waited {took:?}");
}
