// Release builds are GUI-subsystem so no console window opens next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use eframe::egui::ViewportBuilder;
use eframe::egui_wgpu::{SurfaceConfig, WgpuConfiguration, WgpuSetup};
use eframe::wgpu;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::single_instance::{self, Claim};
use neumann_control_app::spawner::CoreSpawner;
use neumann_control_app::system_audio::CoreAudio;
use neumann_control_app::system_language;
use neumann_control_app::texts;
use neumann_control_app::window_placement::{first_window_size, place_window, WINDOW_MIN_SIZE};
use neumann_control_core::settings::{self, LOG_FILE, SETTINGS_FILE};
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};

/// Title bar and taskbar; the executable's own icon comes from `build.rs`.
const WINDOW_ICON: &[u8] = include_bytes!("../assets/brand/icon.png");

// Our crates only: wgpu logs the graphics adapters with their bus ids at info level.
const LOG_TARGET_PREFIX: &str = "neumann_control";

/// eframe's default (vsync via Fifo, two queued frames) waits on the remote display's vblank under
/// Remote Desktop, which ticks at 10-30 Hz there; Mailbox never tears and does not wait. DX12 is
/// pinned because it always offers Mailbox (wgpu-hal), and egui-wgpu does not check the mode.
fn wgpu_options() -> WgpuConfiguration {
    let mut options = WgpuConfiguration {
        surface: SurfaceConfig {
            present_mode: wgpu::PresentMode::Mailbox,
            desired_maximum_frame_latency: Some(1),
        },
        ..WgpuConfiguration::default()
    };
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
    }
    options
}

fn window_icon() -> Result<eframe::egui::IconData, Box<dyn Error>> {
    Ok(eframe::icon_data::from_png_bytes(WINDOW_ICON)?)
}

/// The app still runs without a log, e.g. from a read-only folder.
fn start_log(directory: &Path) {
    let Ok(file) = File::create(directory.join(LOG_FILE)) else {
        return;
    };
    let config = ConfigBuilder::new()
        .add_filter_allow_str(LOG_TARGET_PREFIX)
        .build();
    let _ = WriteLogger::init(LevelFilter::Info, config, file);
}

fn main() -> Result<(), Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or("the executable has no parent folder")?;
    // Before the log is started: it would empty the running app's log.
    let lock = single_instance::claim(directory);
    if let Claim::Taken = lock {
        single_instance::show_running(texts::WINDOW_TITLE);
        return Ok(());
    }
    start_log(directory);
    if let Claim::Unavailable(error) = &lock {
        log::warn!("no instance lock next to the executable ({})", error.kind());
    }
    let settings_path = directory.join(SETTINGS_FILE);
    let saved = settings::load_checked(&settings_path);
    let saved_window = saved.settings.window_of_mode();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(texts::WINDOW_TITLE)
            .with_icon(Arc::new(window_icon()?))
            .with_inner_size(first_window_size(saved.settings.desk.compact))
            .with_min_inner_size(WINDOW_MIN_SIZE),
        centered: true,
        wgpu_options: wgpu_options(),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        texts::WINDOW_TITLE,
        options,
        Box::new(move |creation_context| {
            let window = creation_context.winit_window();
            if let (Some(window), Some(saved)) = (window, saved_window) {
                place_window(window, saved);
            }
            let context = creation_context.egui_ctx.clone();
            let spawner = Box::new(CoreSpawner::new(context.clone()));
            let language = system_language::detect();
            let audio = Box::new(CoreAudio::new());
            let app =
                NeumannApp::with_settings(&context, settings_path, saved, spawner, audio, language);
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}
