//! The "?" menu: the manual next to the exe, and "Über" with its update check, both through a
//! stand-in for the browser and GitHub.

use std::sync::Arc;

use crate::support::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::help::{Release, MANUAL_FILE, REPOSITORY_URL};
use neumann_control_app::texts;

fn app_with(help: &FakeHelp) -> (tempfile::TempDir, Harness<'static, NeumannApp>) {
    let directory = tempfile::tempdir().unwrap();
    let path = settings_file(directory.path(), Vec::new());
    let mut harness = harness(path, Vec::new());
    harness.state_mut().set_help(Arc::new(help.clone()));
    harness.run_steps(1);
    (directory, harness)
}

fn choose_from_help(harness: &mut Harness<'static, NeumannApp>, item: &str) {
    harness.get_by_label(texts::HELP.get()).click();
    harness.run_steps(2);
    harness.get_by_label(item).click();
    harness.run_steps(3);
}

#[test]
fn the_manual_opens_from_the_exes_folder() {
    let help = FakeHelp::default();
    let (directory, mut harness) = app_with(&help);
    let manual = directory.path().join(MANUAL_FILE);
    std::fs::write(&manual, "<html></html>").unwrap();
    choose_from_help(&mut harness, texts::MANUAL.get());
    let opened = help.opened.lock().unwrap().clone();
    assert_eq!(opened, vec![manual.to_string_lossy().into_owned()]);
}

#[test]
fn a_missing_manual_is_said_and_nothing_opened() {
    let help = FakeHelp::default();
    let (_directory, mut harness) = app_with(&help);
    choose_from_help(&mut harness, texts::MANUAL.get());
    harness.get_by_label(texts::MANUAL_MISSING.get());
    assert!(help.opened.lock().unwrap().is_empty());
}

#[test]
fn about_tells_when_no_release_is_published() {
    let (_directory, mut harness) = app_with(&FakeHelp::default());
    choose_from_help(&mut harness, texts::ABOUT.get());
    harness.get_by_label(&texts::version(env!("CARGO_PKG_VERSION")));
    harness.get_by_label(texts::CHECK_FOR_UPDATES.get()).click();
    wait_for_label(&mut harness, texts::NO_RELEASE.get());
}

#[test]
fn a_newer_release_links_to_its_page() {
    let page = format!("{REPOSITORY_URL}/releases/tag/v99.0.0");
    let help = FakeHelp {
        release: Some(Release {
            tag: "v99.0.0".to_owned(),
            url: page.clone(),
        }),
        ..FakeHelp::default()
    };
    let (_directory, mut harness) = app_with(&help);
    choose_from_help(&mut harness, texts::ABOUT.get());
    harness.get_by_label(texts::CHECK_FOR_UPDATES.get()).click();
    wait_for_label(&mut harness, &texts::update_available("v99.0.0"));
    harness.get_by_label(texts::OPEN_RELEASE_PAGE.get()).click();
    harness.run_steps(2);
    assert_eq!(*help.opened.lock().unwrap(), vec![page]);
    harness.get_by_label(texts::CLOSE.get()).click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label(texts::CHECK_FOR_UPDATES.get())
        .is_none());
}

#[test]
fn the_running_version_is_current_against_itself() {
    let help = FakeHelp {
        release: Some(Release {
            tag: format!("v{}", env!("CARGO_PKG_VERSION")),
            url: REPOSITORY_URL.to_owned(),
        }),
        ..FakeHelp::default()
    };
    let (_directory, mut harness) = app_with(&help);
    choose_from_help(&mut harness, texts::ABOUT.get());
    harness.get_by_label(texts::CHECK_FOR_UPDATES.get()).click();
    wait_for_label(&mut harness, texts::UP_TO_DATE.get());
}
