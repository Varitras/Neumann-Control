//! The desk's keyboard shortcuts: in the window with Ctrl, and (when switched on) from any
//! program with Ctrl+Alt, so the global ones do not take Ctrl+arrow from other programs.

use eframe::egui::{InputState, Key, KeyboardShortcut, Modifiers};

/// How far one press moves the master.
pub const MASTER_STEP_DB: f64 = 1.0;
/// How far dim lowers every speaker.
pub const DIM_DB: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskShortcut {
    MasterUp,
    MasterDown,
    MuteAll,
    Dim,
}

/// Each shortcut's key, the same in the window and from other programs.
pub const SHORTCUT_KEYS: [(DeskShortcut, Key); 4] = [
    (DeskShortcut::MasterUp, Key::ArrowUp),
    (DeskShortcut::MasterDown, Key::ArrowDown),
    (DeskShortcut::MuteAll, Key::M),
    (DeskShortcut::Dim, Key::D),
];

/// The shortcuts pressed in the window this frame, taken out of the input.
pub fn pressed(input: &mut InputState) -> Vec<DeskShortcut> {
    SHORTCUT_KEYS
        .into_iter()
        .filter(|(_, key)| input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, *key)))
        .map(|(shortcut, _)| shortcut)
        .collect()
}
