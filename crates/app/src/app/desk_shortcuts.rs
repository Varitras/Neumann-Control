//! What the desk's shortcuts do: the master moves every speaker, mute all mutes or unmutes them
//! all, dim lowers them all and, pressed again, sets each back to the level it had.

use std::sync::Arc;

use neumann_control_core::catalog::Catalog;

use super::desk::{all_muted, level_of};
use super::NeumannApp;
use crate::shortcuts::{DeskShortcut, DIM_DB, MASTER_STEP_DB};
use crate::state::SpeakerKey;
use crate::writes::Intent;

/// Each speaker's level when dim went on, to go back to exactly.
pub(super) type DimmedLevels = Vec<(SpeakerKey, f64)>;

impl NeumannApp {
    pub(super) fn apply_shortcut(&mut self, shortcut: DeskShortcut, now: f64) {
        let catalog = Catalog::embedded();
        let everyone: Vec<usize> = (0..self.speakers.len()).collect();
        match shortcut {
            DeskShortcut::MasterUp => {
                self.apply_shared(&everyone, Some(MASTER_STEP_DB), None, catalog, now);
            }
            DeskShortcut::MasterDown => {
                self.apply_shared(&everyone, Some(-MASTER_STEP_DB), None, catalog, now);
            }
            DeskShortcut::MuteAll => {
                let states = self.states(&everyone);
                let mute = all_muted(&states) != Some(true);
                self.apply_shared(&everyone, None, Some(mute), catalog, now);
            }
            DeskShortcut::Dim => self.toggle_dim(&everyone, catalog, now),
        }
    }

    /// Listens while the settings ask for it, and hands out what was pressed in other programs.
    pub(super) fn global_shortcuts(&mut self) -> Vec<DeskShortcut> {
        if !self.settings.desk.global_shortcuts {
            self.global_listener = None;
            self.global_keys_taken = false;
            return Vec::new();
        }
        if self.global_listener.is_none() && !self.global_keys_taken {
            self.global_listener = self.global_keys.listen(Arc::clone(&self.wake));
            self.global_keys_taken = self.global_listener.is_none();
        }
        self.global_listener
            .as_ref()
            .map(|listener| listener.pressed.try_iter().collect())
            .unwrap_or_default()
    }

    pub fn is_dimmed(&self) -> bool {
        self.dimmed.is_some()
    }

    pub(super) fn toggle_dim(&mut self, everyone: &[usize], catalog: &Catalog, now: f64) {
        let Some(dimmed) = self.dimmed.take() else {
            let levels = self
                .speakers
                .iter()
                .filter_map(|speaker| Some((speaker.state.key.clone(), level_of(&speaker.state)?)))
                .collect();
            self.dimmed = Some(levels);
            self.apply_shared(everyone, Some(-DIM_DB), None, catalog, now);
            return;
        };
        for (key, level) in dimmed {
            let index = self
                .speakers
                .iter()
                .position(|speaker| speaker.state.key == key);
            if let Some(index) = index {
                self.request_and_send(index, Intent::Level(level), catalog, now);
            }
        }
    }
}
