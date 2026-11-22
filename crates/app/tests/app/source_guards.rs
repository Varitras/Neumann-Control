//! Rules the compiler cannot check, read from the app's own source.

use std::fs;
use std::path::{Path, PathBuf};

/// egui's context accessors that hold its lock while their closure runs.
const LOCKING_CALLS: &[&str] = &[
    ".input(|",
    ".input_mut(|",
    ".data(|",
    ".data_mut(|",
    ".memory(|",
    ".memory_mut(|",
    ".options(|",
    ".options_mut(|",
    ".output_mut(|",
];
/// What reaches the context from inside such a closure.
const CONTEXT_USES: &[&str] = &["ui.", "ctx.", "context.", ".ctx()"];

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The body of the closure whose call starts at `start` in `text`, up to its closing
/// parenthesis.
fn closure_body(text: &str, start: usize) -> &str {
    let open = start + text[start..].find('(').unwrap() + 1;
    let mut depth = 1;
    for (offset, character) in text[open..].char_indices() {
        depth += match character {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if depth == 0 {
            let body = &text[open..open + offset];
            // After the closure's parameters.
            let parameters_end = body[1..].find('|').map_or(0, |end| end + 2);
            return &body[parameters_end..];
        }
    }
    ""
}

/// Asking the context inside one of its locked closures takes its lock twice. That works until a
/// worker thread waits to request a repaint; then both wait for good (a deadlock seen in the
/// wheel code).
#[test]
fn no_context_closure_reaches_the_context_again() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&source, &mut files);
    let mut nested = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for opener in LOCKING_CALLS {
            for (start, _) in text.match_indices(opener) {
                let body = closure_body(&text, start);
                if CONTEXT_USES.iter().any(|reach| body.contains(reach)) {
                    let line = text[..start].lines().count();
                    nested.push(format!("{}:{line}", file.display()));
                }
            }
        }
    }
    assert!(
        nested.is_empty(),
        "context reached inside its lock: {nested:?}"
    );
}

/// The release script and the app name the manual each in their own language; the app opens
/// the file the script ships.
#[test]
fn the_release_script_ships_the_manual_the_app_opens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("build-release.ps1")).unwrap();
    let quoted = format!("'{}'", neumann_control_app::help::MANUAL_FILE);
    assert!(
        script.matches(&quoted).count() >= 2,
        "{quoted} in build-release.ps1"
    );
}

/// Cargo puts the package version into the identity of every build artifact, so each new version
/// left a full new set in `target/` next to the old one. The product version lives in `VERSION`
/// and Cargo's stays fixed; built from parts so this file does not name it itself.
const CARGO_VERSION: &str = concat!("CARGO_PKG", "_VERSION");
const FIXED_CARGO_VERSION: &str = "version = \"0.0.0\"";

#[test]
fn the_product_version_comes_from_the_version_file() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![crate_root.join("build.rs")];
    rust_files(&crate_root.join("src"), &mut files);
    rust_files(&crate_root.join("tests"), &mut files);
    let reading_cargo: Vec<_> = files
        .iter()
        .filter(|path| fs::read_to_string(path).unwrap().contains(CARGO_VERSION))
        .collect();
    assert!(
        reading_cargo.is_empty(),
        "{CARGO_VERSION} in {reading_cargo:?}"
    );

    let manifest = fs::read_to_string(crate_root.join("../../Cargo.toml")).unwrap();
    let workspace_package = manifest
        .split("[workspace.package]")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .expect("a [workspace.package] section");
    assert!(
        workspace_package
            .lines()
            .any(|line| line.trim() == FIXED_CARGO_VERSION),
        "the workspace version stays {FIXED_CARGO_VERSION}"
    );
}

/// egui's spinner asks for the next frame at once; without vsync (the app presents by Mailbox)
/// that drew several hundred frames a second while a scan ran. Views use `busy::spinner`.
#[test]
fn no_view_draws_egui_s_own_spinner() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&source, &mut files);
    let mut found = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for call in [".spinner()", "Spinner::new"] {
            for (start, _) in text.match_indices(call) {
                let line = text[..start].lines().count();
                found.push(format!("{}:{line}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "egui's spinner drawn at: {found:?}");
}

/// The arguments of the call whose name ends at `start` in `text`, up to its closing parenthesis.
fn call_arguments(text: &str, start: usize) -> &str {
    let open = start + 1;
    let mut depth = 1;
    for (offset, character) in text[open..].char_indices() {
        depth += match character {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if depth == 0 {
            return &text[open..open + offset];
        }
    }
    ""
}

/// Every read of the last report (`.values`) in the views, the app's window code and the
/// modules that plan writes, by file.
/// A gesture starts from `SpeakerState::latest` (a write on its way included); only what is
/// shown as reported, never a base for a write, reads the report. A new read is a decision:
/// add it here only if it is shown and never written from.
const REPORT_READS: &[(&str, usize)] = &[
    // `display`: read-only rows.
    ("views/details.rs", 1),
    // `display` and the mute button's look (its toggle starts from `latest`).
    ("views/strip.rs", 2),
    // A container's band count, which a write does not change.
    ("app/eq.rs", 2),
    // A stored preset's own values, not the speaker's.
    ("presets.rs", 1),
    // The export writes what the speaker reports.
    ("app/eq_files.rs", 1),
    ("app/full_scan.rs", 1),
];

#[test]
fn gestures_start_from_the_latest_values() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    // Folders whole, and the planning modules beside them (`writes.rs` has none).
    let planners = ["presets.rs", "writes.rs"];
    for folder in ["views", "app", ""] {
        let mut files = Vec::new();
        if folder.is_empty() {
            files.extend(planners.iter().map(|planner| source.join(planner)));
        } else {
            rust_files(&source.join(folder), &mut files);
        }
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            let reads = code.matches(".values").count();
            let name = file
                .strip_prefix(&source)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let allowed = REPORT_READS
                .iter()
                .find(|(allowed, _)| *allowed == name)
                .map_or(0, |(_, count)| *count);
            if reads != allowed {
                found.push(format!("{name}: {reads} reads, {allowed} allowed"));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// Without the folder's lock `main` loads the settings untouched: `load_checked` would move
/// or copy the file aside where another app may be using it.
#[test]
fn main_loads_untouched_without_the_lock() {
    let main =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    let load = main
        .split_once("let saved = if may_write {")
        .and_then(|(_, rest)| rest.split_once("};"))
        .map(|(load, _)| load)
        .unwrap_or_default();
    let (writing, untouched) = load.split_once("} else {").unwrap_or_default();
    let guarded = writing.contains("settings::load_checked(")
        && untouched.contains("settings::load_untouched(");
    assert!(
        guarded,
        "main.rs must load with load_untouched when it may not write"
    );
}

/// The log starts at the level the settings ask for: placing the window, before the app
/// existed to set it, wrote a debug line into every normal log.
#[test]
fn main_sets_the_log_level_before_the_window_opens() {
    let main =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    let level = main.find("log_detail::apply(");
    let window = main.find("eframe::run_native(");
    assert!(
        level
            .zip(window)
            .is_some_and(|(level, window)| level < window),
        "main.rs must call log_detail::apply before eframe::run_native"
    );
}

/// An EQ preset is planned and saved from what the speaker will hold once its writes are
/// through, not from its last report: a band write still on its way would otherwise be left
/// out of a load or saved as it was.
#[test]
fn eq_presets_are_planned_from_the_latest_values() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&source, &mut files);
    let mut found = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        // Unit tests hand the planners their own values.
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for call in ["eq_plan(", "capture_eq("] {
            for (start, _) in code.match_indices(call) {
                let defined = code[..start].ends_with("fn ");
                let arguments = call_arguments(code, start + call.len() - 1);
                if !defined && !arguments.contains("latest_values()") {
                    let line = code[..start].lines().count();
                    found.push(format!("{}:{line}", file.display()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "planned from the reported values at: {found:?}"
    );
}
