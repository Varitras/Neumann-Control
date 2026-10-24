//! The window: owns the settings, one worker per speaker and the scan, and wires the views to
//! them. Worker events are drained once per frame; workers wake the window when they have news.

use std::net::SocketAddrV6;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui::{
    self, Context, Id, Key, KeyboardShortcut, Modifiers, Ui, ViewportCommand, ViewportId,
    ViewportInfo,
};
use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::{merge_found, Found};
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::{
    self, Language, LoadOutcome, Loaded, ManualSpeaker, Settings, WindowGeometry,
};
use neumann_control_core::speaker::{Command, Event, Events, SpeakerHandle, Target};

use crate::display;
use crate::file_dialog::{FileDialog, NativeFileDialog};
use crate::groups::{delete_group, forget_member, member_id, rename_member, set_membership};
use crate::help::{Help, UpdateCheck, WindowsHelp, MANUAL_FILE, WIKI_URL};
use crate::meter::MeterState;
use crate::rediscovery::Rediscovery;
use crate::scan_choice::ScanChoice;
use crate::spawner::{ScanResult, SpeakerSpawner};
use crate::state::{AllValuesState, SpeakerKey, SpeakerState, Status};
use crate::status_leds::average_response_time;
use crate::strip_layout;
use crate::system_audio::{AudioDevice, SystemAudio};
use crate::targets::{
    duplicate_speakers, follow_moves, forget, is_found, remember_identity, remember_serial,
    same_speaker, targets,
};
use crate::texts;
use crate::ui_kit::eq_palette::{curve_key, CURVE_KEYS};
use crate::ui_kit::{fonts, theme};
use crate::views::about::{self, AboutAction, UpdateView};
use crate::views::add_address::{self, AddAddressAction, AddAddressForm};
use crate::views::group_dialog::{self, GroupDialogAction};
use crate::views::panel::PanelPlacement;
use crate::views::remove_speaker::{self, RemoveAction};
use crate::views::scan_choice::{self as scan_choice_view, ScanChoiceAction};
use crate::views::settings_dialog::{self, SettingsAction};
use crate::views::toolbar::{self, ToolbarAction, ToolbarFacts};
use crate::views::{all_values, panel, wheel};
use crate::window_fit::WindowFit;

mod compact;
mod desk;
mod eq;
mod eq_files;
mod full_scan;
mod history;
mod preset_dialogs;
mod system_volume;

pub use history::TakeBackNotice;

const DESK_SCROLL_ID: &str = "desk";
const COMPACT_SCROLL_ID: &str = "compact";
const PANEL_VIEWPORT_ID: &str = "device_panel";
const COMPACT_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

struct Speaker {
    state: SpeakerState,
    handle: SpeakerHandle,
    events: Events,
}

/// A speaker's device panel, open in a window of its own.
struct OpenPanel {
    key: SpeakerKey,
    /// Fixed when it opens: the key can change while the speaker connects, the window stays.
    viewport: ViewportId,
    /// Opened again while open: the window comes to the front.
    focus: bool,
    /// Fixed in the first frame the panel is drawn.
    placement: Option<PanelPlacement>,
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
    /// Everything scans reported this session; names and models for the strips.
    found: Vec<Found>,
    scan: Option<Receiver<ScanResult>>,
    /// The running scan was started by the app itself: it opens no dialog and reports no failure.
    scan_silent: bool,
    rediscovery: Rediscovery,
    scan_choice: Option<ScanChoice>,
    notice: Option<texts::Text>,
    /// False when the file at the start could be neither read nor set aside: saving would
    /// overwrite it with what this run holds.
    settings_writable: bool,
    add_address: Option<AddAddressForm>,
    removal: Option<Removal>,
    panels: Vec<OpenPanel>,
    group_draft: Option<desk::GroupDraft>,
    settings_open: bool,
    /// The desk's strips are being customized; not saved, a new start shows the desk as usual.
    editing: bool,
    window: Option<WindowGeometry>,
    /// Without a saved desk window, the window is sized to the desk until the user sizes it.
    window_fit: Option<WindowFit>,
    history: history::HistoryState,
    eq_windows: Vec<eq::EqWindow>,
    preset_dialog: Option<preset_dialogs::PresetDialog>,
    system_audio: Box<dyn SystemAudio>,
    file_dialog: Box<dyn FileDialog>,
    help: Arc<dyn Help>,
    /// The "Über" dialog while it is open, with its update check.
    about: Option<UpdateView>,
    update_check: Option<UpdateCheck>,
    /// The Windows strip's meters, one per channel of the device.
    system_meters: Vec<MeterState>,
    /// The playback devices for the settings dialog, listed when it opens.
    audio_devices: Vec<AudioDevice>,
    /// What "like Windows" means, asked of Windows at the start.
    system_language: Language,
}

impl NeumannApp {
    /// Loads the settings and starts a worker for every remembered speaker.
    pub fn new(
        context: &Context,
        settings_path: PathBuf,
        spawner: Box<dyn SpeakerSpawner>,
        system_audio: Box<dyn SystemAudio>,
        system_language: Language,
    ) -> NeumannApp {
        let loaded = settings::load_checked(&settings_path);
        NeumannApp::with_settings(
            context,
            settings_path,
            loaded,
            spawner,
            system_audio,
            system_language,
        )
    }

    /// With settings already loaded from `settings_path`, as the start does to place the window:
    /// a second load would set a file with values it does not understand aside a second time.
    pub fn with_settings(
        context: &Context,
        settings_path: PathBuf,
        loaded: Loaded,
        spawner: Box<dyn SpeakerSpawner>,
        system_audio: Box<dyn SystemAudio>,
        system_language: Language,
    ) -> NeumannApp {
        let Loaded { settings, outcome } = loaded;
        fonts::install(context);
        theme::apply(context);
        context.set_theme(theme::preference(settings.desk.theme));
        let repaint_context = context.clone();
        let mut app = NeumannApp {
            window: settings.window_of_mode(),
            window_fit: (settings.window.is_none() && !settings.desk.compact)
                .then(WindowFit::default),
            settings_path,
            settings,
            spawner,
            wake: Arc::new(move || repaint_context.request_repaint()),
            speakers: Vec::new(),
            found: Vec::new(),
            scan: None,
            scan_silent: false,
            rediscovery: Rediscovery::default(),
            scan_choice: None,
            notice: texts::settings_notice(outcome),
            settings_writable: outcome != LoadOutcome::Untouchable,
            add_address: None,
            removal: None,
            panels: Vec::new(),
            group_draft: None,
            settings_open: false,
            editing: false,
            history: history::HistoryState::default(),
            eq_windows: Vec::new(),
            preset_dialog: None,
            system_audio,
            file_dialog: Box::new(NativeFileDialog),
            help: Arc::new(WindowsHelp),
            about: None,
            update_check: None,
            system_meters: Vec::new(),
            audio_devices: Vec::new(),
            system_language,
        };
        app.sync_targets();
        app
    }

    /// The dialog the EQ import and export choose their files with; the tests' stand-in.
    pub fn set_file_dialog(&mut self, file_dialog: Box<dyn FileDialog>) {
        self.file_dialog = file_dialog;
    }

    /// What opens the manual and web pages and asks GitHub; the tests' stand-in.
    pub fn set_help(&mut self, help: Arc<dyn Help>) {
        self.help = help;
    }

    /// Starts workers for remembered speakers not shown yet and moves speakers whose entry now
    /// points at another address.
    fn sync_targets(&mut self) {
        for (key, address) in targets(&self.settings.manual) {
            let discovered = self
                .found
                .iter()
                .find(|found| is_found(found, &key, address));
            let existing = self.speakers.iter().position(|speaker| {
                same_speaker(&speaker.state.key, speaker.state.address, &key, address)
            });
            let Some(position) = existing else {
                let mut state = SpeakerState::new(key.clone(), address);
                state.discovered = discovered.cloned();
                if let SpeakerKey::Serial(serial) = &key {
                    state.remembered = self.settings.known.get(serial).cloned();
                }
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
        speaker.state.forget_outstanding();
        speaker.events = events;
        retire(std::mem::replace(&mut speaker.handle, handle));
    }

    fn drain_events(&mut self, now: f64) {
        let mut connected = Vec::new();
        let mut renamed = Vec::new();
        for speaker in &mut self.speakers {
            while let Ok(event) = speaker.events.try_recv() {
                if let Event::Connected(identity) = &event {
                    connected.push((identity.clone(), speaker.state.address));
                }
                let before = speaker.state.key.clone();
                speaker.state.apply(event, now);
                if speaker.state.key != before {
                    renamed.push((before, speaker.state.key.clone()));
                }
            }
        }
        if connected.is_empty() {
            return;
        }
        let mut changed = false;
        for (identity, address) in &connected {
            changed |= remember_serial(&mut self.settings.manual, &identity.serial, *address);
            changed |= remember_identity(&mut self.settings.known, identity);
        }
        for (before, after) in &renamed {
            changed |= self.follow_key(before, after);
        }
        if changed {
            self.save_settings();
        }
        self.drop_duplicate_speakers();
    }

    /// A speaker's key changed (its serial became known): its group, its place in the order and
    /// its open panel go with it.
    fn follow_key(&mut self, before: &SpeakerKey, after: &SpeakerKey) -> bool {
        for panel in self.panels.iter_mut().filter(|panel| panel.key == *before) {
            panel.key = after.clone();
        }
        let (old, new) = (member_id(before), member_id(after));
        let settings = &mut self.settings;
        rename_member(&mut settings.groups, &mut settings.order, &old, &new)
    }

    /// One speaker reached at two addresses shows up twice until both connected.
    fn drop_duplicate_speakers(&mut self) {
        let keys: Vec<SpeakerKey> = self
            .speakers
            .iter()
            .map(|speaker| speaker.state.key.clone())
            .collect();
        for position in duplicate_speakers(&keys).into_iter().rev() {
            let speaker = self.speakers.remove(position);
            log::info!(
                "{} is a speaker already shown",
                mask_host(&speaker.state.address)
            );
            retire(speaker.handle);
        }
    }

    /// Starts a silent scan when the start or a speaker that stays unreachable calls for one; it
    /// follows remembered speakers to their new address and adds none.
    fn rediscover(&mut self, now: f64) {
        let followable = self
            .settings
            .manual
            .iter()
            .any(|entry| entry.serial.is_some());
        let unreachable_for = self
            .speakers
            .iter()
            .filter_map(|speaker| speaker.state.unreachable_since)
            .map(|since| now - since)
            .reduce(f64::max);
        let scanning = self.scan.is_some();
        if self
            .rediscovery
            .due(now, followable, unreachable_for, scanning)
        {
            log::info!("looking for remembered speakers again");
            self.scan = Some(self.spawner.scan());
            self.scan_silent = true;
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
        let silent = std::mem::take(&mut self.scan_silent);
        let found = match result {
            Ok(found) => merge_found(found),
            Err(reason) => {
                log::warn!("scan failed: {reason}");
                if !silent {
                    self.show_scan_failure();
                }
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
        if let Some(choice) = &self.scan_choice {
            self.scan_choice = Some(choice.again(&found, &self.shown_speakers()));
        }
    }

    /// The dialog shows the failure; after it was cancelled, the toolbar does.
    fn show_scan_failure(&mut self) {
        match &mut self.scan_choice {
            Some(choice) => *choice = ScanChoice::Failed,
            None => self.notice = Some(texts::SCAN_FAILED),
        }
    }

    fn shown_speakers(&self) -> Vec<(SpeakerKey, SocketAddrV6)> {
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

    /// Stops the speaker's worker and forgets it in the settings: its manual entry, its group and
    /// its place in the order.
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
        let ids = [member_id(&key), member_id(&SpeakerKey::Address(address))];
        let settings = &mut self.settings;
        let mut forgotten = forget_member(&mut settings.groups, &mut settings.order, &ids);
        if let SpeakerKey::Serial(serial) = &key {
            forgotten |= settings.known.remove(serial).is_some();
        }
        if forget(&mut settings.manual, &key, address) | forgotten {
            self.save_settings();
        }
    }

    fn save_settings(&mut self) {
        if !self.settings_writable {
            self.notice = Some(texts::SETTINGS_UNTOUCHABLE);
            return;
        }
        if let Err(error) = settings::save(&self.settings, &self.settings_path) {
            log::warn!("settings could not be saved ({})", error.kind());
            self.notice = Some(texts::SETTINGS_NOT_SAVED);
        }
    }

    fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame) {
        let facts = ToolbarFacts {
            compact: self.settings.desk.compact,
            editing: self.editing,
            scanning: self.scan.is_some(),
            speaker_count: self.speakers.len(),
            notice: self.notice,
            average_response_time: self.average_response_time(),
        };
        match toolbar::show(ui, &facts) {
            ToolbarAction::Scan => {
                self.notice = None;
                self.scan = Some(self.spawner.scan());
                self.scan_silent = false;
                self.scan_choice = Some(ScanChoice::Scanning);
            }
            ToolbarAction::AddAddress => self.add_address = Some(AddAddressForm::default()),
            ToolbarAction::AddGroup => self.add_group(),
            ToolbarAction::Settings => {
                self.settings_open = true;
                self.audio_devices = self.system_audio.devices();
            }
            ToolbarAction::History => self.open_history(),
            ToolbarAction::ToggleCompact => self.toggle_compact(frame),
            ToolbarAction::ToggleEditing => self.editing = !self.editing,
            ToolbarAction::Manual => self.open_manual(),
            ToolbarAction::Wiki => self.open_web(WIKI_URL),
            ToolbarAction::About => self.about = Some(UpdateView::NotAsked),
            ToolbarAction::None => {}
        }
        if ui.input_mut(|input| input.consume_shortcut(&COMPACT_SHORTCUT)) {
            self.toggle_compact(frame);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if self.settings.desk.compact {
                self.show_compact(ui);
            } else {
                self.show_desk(ui);
            }
        });
        self.show_panels(ui.ctx());
        self.show_dialog(ui);
        self.show_scan_choice(ui);
        self.show_removal(ui);
        self.show_about(ui);
        self.show_preset_dialog(ui);
        self.show_settings(ui);
        self.show_group_dialog(ui);
        self.show_all_values(ui.ctx());
        self.show_eq_windows(ui.ctx());
        // After the EQ windows: where one is embedded in this window, its Ctrl+Z comes first.
        self.undo_shortcut(ui);
        self.show_history(ui);
        self.show_take_back_question(ui);
        let now = ui.input(|input| input.time);
        self.finish_history_frame(now);
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

    /// Opens the speaker's panel, or brings it to the front when it is open.
    fn open_panel(&mut self, key: SpeakerKey) {
        if let Some(open) = self.panels.iter_mut().find(|panel| panel.key == key) {
            open.focus = true;
            return;
        }
        let viewport = ViewportId::from_hash_of(Id::unique(PANEL_VIEWPORT_ID).with(&key));
        self.panels.push(OpenPanel {
            key,
            viewport,
            focus: false,
            placement: None,
        });
    }

    /// Every open device panel; one closes when its speaker is gone or unreachable.
    fn show_panels(&mut self, context: &Context) {
        let speakers = &self.speakers;
        self.panels.retain(|panel| {
            speakers.iter().any(|speaker| {
                speaker.state.key == panel.key && speaker.state.status != Status::Unreachable
            })
        });
        let mut closed = Vec::new();
        for panel in 0..self.panels.len() {
            if self.show_panel(context, panel) {
                closed.push(panel);
            }
        }
        for panel in closed.into_iter().rev() {
            self.panels.remove(panel);
        }
    }

    /// One panel; true when it is to close.
    fn show_panel(&mut self, context: &Context, panel: usize) -> bool {
        let open = &mut self.panels[panel];
        let Some(index) = self
            .speakers
            .iter()
            .position(|speaker| speaker.state.key == open.key)
        else {
            return true;
        };
        let viewport = open.viewport;
        let placement = *open.placement.get_or_insert_with(|| {
            let (main, monitor) = context.input_for(ViewportId::ROOT, |input| {
                (input.viewport().outer_rect, input.viewport().monitor_size)
            });
            PanelPlacement::new(main, monitor)
        });
        if std::mem::take(&mut open.focus) {
            context.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        }
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
        let product = state.product().to_owned();
        let stored = self.settings.desk.strip_layouts.get(&product);
        let layout = state
            .identity
            .is_some()
            .then(|| strip_layout::resolve(stored, &product, catalog));
        let response = panel::show(
            context,
            viewport,
            placement,
            state,
            catalog,
            &choice,
            layout.as_ref(),
        );
        if let Some(change) = &response.layout {
            self.change_layout(&product, change, catalog);
        }
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
        let key = speaker.key.clone();
        if response.open_eq {
            self.open_eq(key.clone());
        }
        if response.save_preset {
            self.open_preset_save(key.clone());
        }
        if response.load_preset {
            self.open_preset_load(key);
        }
        response.close || response.remove
    }

    fn show_group_dialog(&mut self, ui: &mut Ui) {
        let Some(draft) = &mut self.group_draft else {
            return;
        };
        match group_dialog::show(ui, &mut draft.name, draft.deleting) {
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
            GroupDialogAction::Delete => draft.deleting = true,
            GroupDialogAction::CancelDelete => draft.deleting = false,
            GroupDialogAction::ConfirmDelete => {
                delete_group(&mut self.settings.groups, draft.index);
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
        let labels = curve_labels();
        match settings_dialog::show(ui, &mut self.settings, &self.audio_devices, &labels) {
            SettingsAction::Changed => {
                ui.ctx()
                    .set_theme(theme::preference(self.settings.desk.theme));
                self.save_settings();
            }
            SettingsAction::Close => self.settings_open = false,
            SettingsAction::None => {}
        }
    }

    /// From the exe's folder, where the release puts it next to the settings.
    fn open_manual(&mut self) {
        let manual = self.settings_path.with_file_name(MANUAL_FILE);
        let opened = manual.is_file() && self.help.open(&manual.to_string_lossy());
        if !opened {
            log::warn!("the manual could not be opened");
            self.notice = Some(texts::MANUAL_MISSING);
        }
    }

    fn show_about(&mut self, ui: &mut Ui) {
        let Some(view) = &mut self.about else {
            return;
        };
        if let Some(result) = self.update_check.as_ref().and_then(UpdateCheck::result) {
            *view = UpdateView::Done(result);
            self.update_check = None;
        }
        match about::show(ui, view) {
            AboutAction::None => {}
            AboutAction::Close => {
                // A check still out finishes on its own; its answer is dropped.
                self.about = None;
                self.update_check = None;
            }
            AboutAction::CheckForUpdates => {
                *view = UpdateView::Checking;
                let wake = Arc::clone(&self.wake);
                self.update_check = Some(UpdateCheck::start(Arc::clone(&self.help), wake));
            }
            AboutAction::Open(url) => self.open_web(&url),
        }
    }

    fn open_web(&self, url: &str) {
        if !self.help.open(url) {
            log::warn!("the browser could not be started");
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
        match scan_choice_view::show(ui, choice, self.scan.is_some()) {
            ScanChoiceAction::None => {}
            ScanChoiceAction::ScanAgain => {
                if self.scan.is_none() {
                    self.scan = Some(self.spawner.scan());
                }
                // A silent scan that runs already now reports to the dialog, failures too.
                self.scan_silent = false;
            }
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
/// The local time for the history and the scan report; UTC when the offset cannot be told.
fn local_now() -> time::OffsetDateTime {
    time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc())
}

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
        self.write_scan_reports();
        self.rediscover(now);
        self.poll_scan();
        let pixels_per_point = context.pixels_per_point();
        let geometry = context.input(|input| window_geometry(input.viewport(), pixels_per_point));
        self.window = geometry.or(self.window);
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        texts::set_language(self.settings.desk.language.resolve(self.system_language));
        display::set_delay_unit(self.settings.desk.delay_unit);
        wheel::track_burst(ui.ctx());
        self.keep_system_meters_moving(ui.ctx());
        self.show(ui, frame);
    }

    fn on_exit(&mut self) {
        // All workers hear Stop first, so the joins in Drop overlap instead of queueing.
        for speaker in &self.speakers {
            speaker.handle.send(Command::Stop);
        }
        self.speakers.clear();
        if let Some(window) = self.window {
            self.settings.remember_window(window);
            self.save_settings();
        }
    }
}

/// The EQ curves the settings offer colours for, named as the EQ window's tabs name them.
fn curve_labels() -> Vec<(String, String)> {
    let catalog = Catalog::embedded();
    CURVE_KEYS
        .iter()
        .filter_map(|key| {
            let container = catalog
                .eq
                .iter()
                .find(|container| curve_key(&container.path) == *key)?;
            Some((
                key.to_string(),
                container.label(texts::language()).to_owned(),
            ))
        })
        .collect()
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
