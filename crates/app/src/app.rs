//! The window: owns the settings, one worker per speaker and the scan, and wires the views to
//! them. Worker events are drained once per frame; workers wake the window when they have news.

use std::net::SocketAddrV6;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui::{self, Context, Pos2, Ui, ViewportInfo};
use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::{merge_found, Found};
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::{self, ManualSpeaker, Settings, WindowGeometry};
use neumann_control_core::speaker::{Command, Event, Events, SpeakerHandle, Target};

use crate::groups::set_membership;
use crate::scan_choice::ScanChoice;
use crate::spawner::{ScanResult, SpeakerSpawner};
use crate::state::{AllValuesState, SpeakerKey, SpeakerState, Status};
use crate::status_leds::average_response_time;
use crate::targets::{duplicate_cards, follow_moves, forget, remember_serial, targets};
use crate::texts;
use crate::ui_kit::{fonts, theme};
use crate::views::add_address::{self, AddAddressAction, AddAddressForm};
use crate::views::group_dialog::{self, GroupDialogAction};
use crate::views::remove_speaker::{self, RemoveAction};
use crate::views::scan_choice::{self as scan_choice_view, ScanChoiceAction};
use crate::views::settings_dialog::{self, SettingsAction};
use crate::views::toolbar::{self, ToolbarAction, ToolbarFacts};
use crate::views::{all_values, panel};

mod desk;

const DESK_SCROLL_ID: &str = "desk";

struct Speaker {
    state: SpeakerState,
    handle: SpeakerHandle,
    events: Events,
}

/// The speaker whose device panel is open and where it opens.
struct OpenPanel {
    key: SpeakerKey,
    anchor: Pos2,
    /// The opening click is still in this frame's input and must not close the panel.
    opened_now: bool,
}

/// The speaker "Entfernen" was clicked for, while the confirmation is open. Key and address both,
/// because connecting can change the key meanwhile.
struct Removal {
    key: SpeakerKey,
    address: SocketAddrV6,
    name: String,
}

pub struct NeumannApp {
    settings_path: PathBuf,
    settings: Settings,
    spawner: Box<dyn SpeakerSpawner>,
    wake: Arc<dyn Fn() + Send + Sync>,
    speakers: Vec<Speaker>,
    /// Everything scans reported this session; names and models for the cards.
    found: Vec<Found>,
    scan: Option<Receiver<ScanResult>>,
    scan_choice: Option<ScanChoice>,
    notice: Option<texts::Text>,
    add_address: Option<AddAddressForm>,
    removal: Option<Removal>,
    panel: Option<OpenPanel>,
    group_draft: Option<desk::GroupDraft>,
    settings_open: bool,
    window: Option<WindowGeometry>,
}

impl NeumannApp {
    /// Loads the settings and starts a worker for every remembered speaker.
    pub fn new(
        context: &Context,
        settings_path: PathBuf,
        spawner: Box<dyn SpeakerSpawner>,
    ) -> NeumannApp {
        fonts::install(context);
        theme::apply(context);
        let settings = settings::load(&settings_path);
        context.set_theme(theme::preference(settings.desk.theme));
        let repaint_context = context.clone();
        let mut app = NeumannApp {
            window: settings.window,
            settings_path,
            settings,
            spawner,
            wake: Arc::new(move || repaint_context.request_repaint()),
            speakers: Vec::new(),
            found: Vec::new(),
            scan: None,
            scan_choice: None,
            notice: None,
            add_address: None,
            removal: None,
            panel: None,
            group_draft: None,
            settings_open: false,
        };
        app.sync_targets();
        app
    }

    /// Starts workers for remembered speakers without a card and moves cards whose entry now
    /// points at another address.
    fn sync_targets(&mut self) {
        for (key, address) in targets(&self.settings.manual) {
            let discovered = self.found.iter().find(|found| found.address == address);
            let existing = self
                .speakers
                .iter()
                .position(|speaker| speaker.state.key == key || speaker.state.address == address);
            let Some(position) = existing else {
                let mut state = SpeakerState::new(key.clone(), address);
                state.discovered = discovered.cloned();
                let (handle, events) = self.spawn(&key, address);
                self.speakers.push(Speaker {
                    state,
                    handle,
                    events,
                });
                continue;
            };
            if discovered.is_some() {
                self.speakers[position].state.discovered = discovered.cloned();
            }
            if self.speakers[position].state.address != address {
                self.respawn(position, &key, address);
            }
        }
    }

    fn spawn(&self, key: &SpeakerKey, address: SocketAddrV6) -> (SpeakerHandle, Events) {
        let expected_serial = match key {
            SpeakerKey::Serial(serial) => Some(serial.clone()),
            SpeakerKey::Address(_) => None,
        };
        let target = Target {
            address,
            expected_serial,
        };
        self.spawner.spawn(target, Arc::clone(&self.wake))
    }

    fn respawn(&mut self, position: usize, key: &SpeakerKey, address: SocketAddrV6) {
        log::info!("speaker moved to {}", mask_host(&address));
        let (handle, events) = self.spawn(key, address);
        let speaker = &mut self.speakers[position];
        speaker.state.address = address;
        speaker.events = events;
        retire(std::mem::replace(&mut speaker.handle, handle));
    }

    fn drain_events(&mut self, now: f64) {
        let mut connected = Vec::new();
        for speaker in &mut self.speakers {
            while let Ok(event) = speaker.events.try_recv() {
                if let Event::Connected(identity) = &event {
                    connected.push((identity.serial.clone(), speaker.state.address));
                }
                speaker.state.apply(event, now);
            }
        }
        if connected.is_empty() {
            return;
        }
        let mut remembered = false;
        for (serial, address) in &connected {
            remembered |= remember_serial(&mut self.settings.manual, serial, *address);
        }
        if remembered {
            self.save_settings();
        }
        self.drop_duplicate_cards();
    }

    /// One speaker reached at two addresses shows up as two cards until both connected.
    fn drop_duplicate_cards(&mut self) {
        let keys: Vec<SpeakerKey> = self
            .speakers
            .iter()
            .map(|speaker| speaker.state.key.clone())
            .collect();
        for position in duplicate_cards(&keys).into_iter().rev() {
            let speaker = self.speakers.remove(position);
            log::info!(
                "{} is a speaker already shown",
                mask_host(&speaker.state.address)
            );
            retire(speaker.handle);
        }
    }

    fn poll_scan(&mut self) {
        let Some(receiver) = &self.scan else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the scan ended without a result".to_owned()),
        };
        self.scan = None;
        let found = match result {
            Ok(found) => merge_found(found),
            Err(reason) => {
                log::warn!("scan failed: {reason}");
                self.show_scan_failure();
                return;
            }
        };
        log::info!("scan found {} speakers", found.len());
        let mut all = std::mem::take(&mut self.found);
        all.extend(found.iter().cloned());
        self.found = merge_found(all);
        if follow_moves(&mut self.settings.manual, &found) {
            self.save_settings();
        }
        self.sync_targets();
        if self.scan_choice.is_some() {
            self.scan_choice = Some(ScanChoice::offer(&found, &self.cards()));
        }
    }

    /// The dialog shows the failure; after it was cancelled, the toolbar does.
    fn show_scan_failure(&mut self) {
        match &mut self.scan_choice {
            Some(choice) => *choice = ScanChoice::Failed,
            None => self.notice = Some(texts::SCAN_FAILED),
        }
    }

    fn cards(&self) -> Vec<(SpeakerKey, SocketAddrV6)> {
        self.speakers
            .iter()
            .map(|speaker| (speaker.state.key.clone(), speaker.state.address))
            .collect()
    }

    /// The dialogs already refused unparsable and listed addresses.
    fn add_speakers(&mut self, entries: Vec<ManualSpeaker>) {
        if entries.is_empty() {
            return;
        }
        for entry in &entries {
            if let Ok(address) = parse_address(&entry.address, entry.port) {
                log::info!("speaker added at {}", mask_host(&address));
            }
        }
        self.settings.manual.extend(entries);
        self.save_settings();
        self.sync_targets();
    }

    /// Stops the card's worker and forgets every settings entry of its speaker.
    fn remove_speaker(&mut self, removal: Removal) {
        let position = self.speakers.iter().position(|speaker| {
            speaker.state.key == removal.key || speaker.state.address == removal.address
        });
        let (key, address) = match position {
            Some(position) => {
                let speaker = self.speakers.remove(position);
                retire(speaker.handle);
                (speaker.state.key, speaker.state.address)
            }
            None => (removal.key, removal.address),
        };
        log::info!("speaker at {} removed", mask_host(&address));
        if forget(&mut self.settings.manual, &key, address) {
            self.save_settings();
        }
    }

    fn save_settings(&mut self) {
        if let Err(error) = settings::save(&self.settings, &self.settings_path) {
            log::warn!("settings could not be saved ({})", error.kind());
            self.notice = Some(texts::SETTINGS_NOT_SAVED);
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        let facts = ToolbarFacts {
            scanning: self.scan.is_some(),
            speaker_count: self.speakers.len(),
            notice: self.notice,
            average_response_time: self.average_response_time(),
        };
        match toolbar::show(ui, &facts) {
            ToolbarAction::Scan => {
                self.notice = None;
                self.scan = Some(self.spawner.scan());
                self.scan_choice = Some(ScanChoice::Scanning);
            }
            ToolbarAction::AddAddress => self.add_address = Some(AddAddressForm::default()),
            ToolbarAction::AddGroup => self.add_group(),
            ToolbarAction::Settings => self.settings_open = true,
            ToolbarAction::None => {}
        }
        egui::CentralPanel::default().show(ui, |ui| self.show_desk(ui));
        self.show_panel(ui.ctx());
        self.show_dialog(ui);
        self.show_scan_choice(ui);
        self.show_removal(ui);
        self.show_settings(ui);
        self.show_group_dialog(ui);
        self.show_all_values(ui.ctx());
    }

    fn show_all_values(&mut self, context: &Context) {
        let catalog = Catalog::embedded();
        for speaker in &mut self.speakers {
            if speaker.state.all_values == AllValuesState::Idle {
                continue;
            }
            for command in all_values::show(context, &mut speaker.state, catalog) {
                speaker.handle.send(command);
            }
        }
    }

    /// The open device panel; it closes when its speaker is gone or unreachable.
    fn show_panel(&mut self, context: &Context) {
        let Some(open) = &mut self.panel else {
            return;
        };
        let index = self
            .speakers
            .iter()
            .position(|speaker| speaker.state.key == open.key)
            .filter(|index| self.speakers[*index].state.status != Status::Unreachable);
        let Some(index) = index else {
            self.panel = None;
            return;
        };
        let (anchor, opened_now) = (open.anchor, open.opened_now);
        open.opened_now = false;
        let catalog = Catalog::embedded();
        let ids = self.member_ids();
        let names: Vec<String> = self
            .settings
            .groups
            .iter()
            .map(|group| group.name.clone())
            .collect();
        let current = desk::group_of(&self.settings.groups, &ids[index]);
        let linked = current.map_or(Vec::new(), |group| {
            self.settings.groups[group].linked.clone()
        });
        let choice = panel::GroupChoice {
            names: &names,
            current,
            linked: &linked,
        };
        let state = &mut self.speakers[index].state;
        let response = panel::show(context, state, catalog, anchor, opened_now, &choice);
        let now = context.input(|input| input.time);
        for command in response.commands {
            self.send_with_links(index, command, catalog, &ids, now);
        }
        if let Some(group) = response.group {
            set_membership(&mut self.settings.groups, &ids[index], group);
            self.save_settings();
        }
        if let Some(linkable) = response.copy_to_group {
            self.copy_to_group(index, linkable, catalog, &ids, now);
        }
        let speaker = &self.speakers[index].state;
        if response.remove {
            self.removal = Some(Removal {
                key: speaker.key.clone(),
                address: speaker.address,
                name: speaker.display_name(),
            });
        }
        if response.close || response.remove {
            self.panel = None;
        }
    }

    fn show_group_dialog(&mut self, ui: &mut Ui) {
        let Some(draft) = &mut self.group_draft else {
            return;
        };
        match group_dialog::show(ui, &mut draft.name) {
            GroupDialogAction::None => {}
            GroupDialogAction::Close => {
                let (index, name) = (draft.index, draft.name.trim().to_owned());
                if let Some(group) = self
                    .settings
                    .groups
                    .get_mut(index)
                    .filter(|_| !name.is_empty())
                {
                    group.name = name;
                }
                self.group_draft = None;
                self.save_settings();
            }
            GroupDialogAction::Delete => {
                let index = draft.index;
                if index < self.settings.groups.len() {
                    self.settings.groups.remove(index);
                }
                self.group_draft = None;
                self.save_settings();
            }
        }
    }

    fn average_response_time(&self) -> Option<std::time::Duration> {
        if !self.settings.desk.show_response_times {
            return None;
        }
        average_response_time(self.speakers.iter().map(|speaker| &speaker.state))
    }

    fn show_settings(&mut self, ui: &mut Ui) {
        if !self.settings_open {
            return;
        }
        match settings_dialog::show(ui, &mut self.settings.desk) {
            SettingsAction::Changed => {
                ui.ctx()
                    .set_theme(theme::preference(self.settings.desk.theme));
                self.save_settings();
            }
            SettingsAction::Close => self.settings_open = false,
            SettingsAction::None => {}
        }
    }

    fn show_removal(&mut self, ui: &mut Ui) {
        let Some(removal) = &self.removal else {
            return;
        };
        match remove_speaker::show(ui, &removal.name) {
            RemoveAction::None => {}
            RemoveAction::Cancel => self.removal = None,
            RemoveAction::Confirm => {
                if let Some(removal) = self.removal.take() {
                    self.remove_speaker(removal);
                }
            }
        }
    }

    fn show_dialog(&mut self, ui: &mut Ui) {
        let Some(form) = &mut self.add_address else {
            return;
        };
        match add_address::show(ui, form, &self.settings.manual) {
            AddAddressAction::None => {}
            AddAddressAction::Cancel => self.add_address = None,
            AddAddressAction::Add(manual) => {
                self.add_address = None;
                self.add_speakers(vec![manual]);
            }
        }
    }

    fn show_scan_choice(&mut self, ui: &mut Ui) {
        let Some(choice) = &mut self.scan_choice else {
            return;
        };
        match scan_choice_view::show(ui, choice) {
            ScanChoiceAction::None => {}
            ScanChoiceAction::Toggle(index) => choice.toggle(index),
            ScanChoiceAction::Cancel => self.scan_choice = None,
            ScanChoiceAction::Add => {
                let chosen = choice.chosen();
                self.scan_choice = None;
                self.add_speakers(chosen);
            }
        }
    }
}

/// Dropping a handle joins its worker, which can sit in a 3 s connect; not on the UI thread.
fn retire(handle: SpeakerHandle) {
    thread::spawn(move || drop(handle));
}

/// Outer position and inner size in physical pixels, as `window_placement` takes them back.
/// `None` while minimized or maximized, so the file keeps the last normal window.
pub fn window_geometry(viewport: &ViewportInfo, pixels_per_point: f32) -> Option<WindowGeometry> {
    let outer = viewport.outer_rect?;
    let inner = viewport.inner_rect?;
    let normal = viewport.minimized != Some(true) && viewport.maximized != Some(true);
    if !normal {
        return None;
    }
    let physical = |points: f32| (points * pixels_per_point).round();
    Some(WindowGeometry {
        x: physical(outer.min.x),
        y: physical(outer.min.y),
        width: physical(inner.width()),
        height: physical(inner.height()),
    })
}

impl eframe::App for NeumannApp {
    fn logic(&mut self, context: &Context, _frame: &mut eframe::Frame) {
        let now = context.input(|input| input.time);
        self.drain_events(now);
        self.poll_scan();
        let pixels_per_point = context.pixels_per_point();
        let geometry = context.input(|input| window_geometry(input.viewport(), pixels_per_point));
        self.window = geometry.or(self.window);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        texts::set_language(self.settings.desk.language);
        self.show(ui);
    }

    fn on_exit(&mut self) {
        // All workers hear Stop first, so the joins in Drop overlap instead of queueing.
        for speaker in &self.speakers {
            speaker.handle.send(Command::Stop);
        }
        self.speakers.clear();
        if self.window.is_some() {
            self.settings.window = self.window;
            self.save_settings();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, Rect};

    fn viewport() -> ViewportInfo {
        ViewportInfo {
            outer_rect: Some(Rect::from_min_size(pos2(-10.0, 20.0), vec2(812.0, 534.0))),
            inner_rect: Some(Rect::from_min_size(pos2(-6.0, 50.0), vec2(800.0, 500.0))),
            ..ViewportInfo::default()
        }
    }

    #[test]
    fn geometry_is_outer_position_and_inner_size_in_physical_pixels() {
        assert_eq!(
            window_geometry(&viewport(), 1.5),
            Some(WindowGeometry {
                x: -15.0,
                y: 30.0,
                width: 1200.0,
                height: 750.0,
            })
        );
    }

    #[test]
    fn minimized_or_maximized_windows_keep_the_last_normal_geometry() {
        let minimized = ViewportInfo {
            minimized: Some(true),
            ..viewport()
        };
        let maximized = ViewportInfo {
            maximized: Some(true),
            ..viewport()
        };
        assert_eq!(window_geometry(&minimized, 1.0), None);
        assert_eq!(window_geometry(&maximized, 1.0), None);
        assert_eq!(window_geometry(&ViewportInfo::default(), 1.0), None);
    }
}
