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
