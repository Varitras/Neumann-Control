//! The desk's keyboard shortcuts: in the window with Ctrl, and (when switched on) from any
//! program with Ctrl+Shift+Alt, so the global ones take neither Ctrl+arrow nor an AltGr character
//! (Windows reports AltGr as Ctrl+Alt) from other programs.

use eframe::egui::{Event, InputState, Key, KeyboardShortcut, Modifiers};

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

impl DeskShortcut {
    /// A switch acts once per press; a held key repeats, which only the master steps follow.
    fn switches(self) -> bool {
        matches!(self, DeskShortcut::MuteAll | DeskShortcut::Dim)
    }
}

/// The shortcuts pressed in the window this frame, taken out of the input. A switch takes all of
/// its key's presses and acts once, and only for a fresh one: the repeats of a held key flipped
/// it again each time, where the global hot keys act once per press.
pub fn pressed(input: &mut InputState) -> Vec<DeskShortcut> {
    SHORTCUT_KEYS
        .into_iter()
        .filter(|(shortcut, key)| {
            let shortcut_key = KeyboardShortcut::new(Modifiers::COMMAND, *key);
            if !shortcut.switches() {
                return input.consume_shortcut(&shortcut_key);
            }
            let fresh = has_fresh_press(input, *key);
            while input.consume_shortcut(&shortcut_key) {}
            fresh
        })
        .map(|(shortcut, _)| shortcut)
        .collect()
}

/// Whether `key` went down this frame, not only repeated as a held key.
fn has_fresh_press(input: &InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            Event::Key { key: pressed_key, pressed: true, repeat: false, modifiers, .. }
                if *pressed_key == key && modifiers.matches_logically(Modifiers::COMMAND)
        )
    })
}
