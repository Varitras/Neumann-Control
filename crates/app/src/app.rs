//! The window: owns the settings, one worker per speaker and the scan, and wires the views to
//! them. Worker events are drained once per frame; workers wake the window when they have news.

use std::net::SocketAddrV6;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui::{self, Context, ScrollArea, Ui, ViewportInfo};
use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::{merge_found, Found};
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::{self, ManualSpeaker, Settings, WindowGeometry};
use neumann_control_core::speaker::{Command, Event, SpeakerHandle, Target};

use crate::spawner::{ScanResult, SpeakerSpawner};
use crate::state::{SpeakerKey, SpeakerState};
use crate::targets::{duplicate_cards, merge_targets, remember_serial};
use crate::texts;
use crate::ui_kit::{fonts, theme};
use crate::views::add_address::{self, AddAddressAction, AddAddressForm};
use crate::views::card;
use crate::views::toolbar::{self, ToolbarAction, ToolbarFacts};

const CARDS_SCROLL_ID: &str = "cards";

struct Speaker {
    state: SpeakerState,
    handle: SpeakerHandle,
    events: Receiver<Event>,
}

pub struct NeumannApp {
    settings_path: PathBuf,
    settings: Settings,
    spawner: Box<dyn SpeakerSpawner>,
    wake: Arc<dyn Fn() + Send + Sync>,
    speakers: Vec<Speaker>,
    found: Vec<Found>,
    scan: Option<Receiver<ScanResult>>,
    notice: Option<&'static str>,
    add_address: Option<AddAddressForm>,
    window: Option<WindowGeometry>,
}

impl NeumannApp {
    /// Loads the settings and starts a worker for every manual speaker.
    pub fn new(
        context: &Context,
        settings_path: PathBuf,
        spawner: Box<dyn SpeakerSpawner>,
    ) -> NeumannApp {
        fonts::install(context);
        theme::apply(context);
        let settings = settings::load(&settings_path);
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
            notice: None,
            add_address: None,
        };
        app.sync_targets();
        app
    }

    /// Starts workers for targets without a card and moves cards whose speaker changed address.
    fn sync_targets(&mut self) {
        for (key, address) in merge_targets(&self.settings.manual, &self.found) {
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

    fn spawn(&self, key: &SpeakerKey, address: SocketAddrV6) -> (SpeakerHandle, Receiver<Event>) {
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
        match result {
            Ok(found) => {
                log::info!("scan found {} speakers", found.len());
                let mut all = std::mem::take(&mut self.found);
                all.extend(found);
                self.found = merge_found(all);
                self.sync_targets();
            }
            Err(reason) => {
                log::warn!("scan failed: {reason}");
                self.notice = Some(texts::SCAN_FAILED);
            }
        }
    }

    /// The dialog already refused unparsable and listed addresses.
    fn add_manual(&mut self, manual: ManualSpeaker) {
        let Ok(address) = parse_address(&manual.address, manual.port) else {
            return;
        };
        log::info!("manual speaker added at {}", mask_host(&address));
        self.settings.manual.push(manual);
        self.save_settings();
        self.sync_targets();
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
        };
        match toolbar::show(ui, &facts) {
            ToolbarAction::Scan => {
                self.notice = None;
                self.scan = Some(self.spawner.scan());
            }
            ToolbarAction::AddAddress => self.add_address = Some(AddAddressForm::default()),
            ToolbarAction::None => {}
        }
        egui::CentralPanel::default().show(ui, |ui| self.show_cards(ui));
        self.show_dialog(ui);
    }

    fn show_cards(&mut self, ui: &mut Ui) {
        if self.speakers.is_empty() {
            ui.weak(texts::NO_SPEAKERS);
            return;
        }
        let catalog = Catalog::embedded();
        ScrollArea::vertical()
            .id_salt(CARDS_SCROLL_ID)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for speaker in &mut self.speakers {
                        let key = speaker.state.key.clone();
                        let commands = ui
                            .push_id(key, |ui| card::show(ui, &mut speaker.state, catalog))
                            .inner;
                        for command in commands {
                            speaker.handle.send(command);
                        }
                    }
                });
            });
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
                self.add_manual(manual);
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
