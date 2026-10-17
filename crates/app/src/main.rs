// Release builds are GUI-subsystem so no console window opens next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::fs::File;
use std::path::Path;

use eframe::egui::{pos2, vec2, ViewportBuilder};
use eframe::egui_wgpu::{SurfaceConfig, WgpuConfiguration, WgpuSetup};
use eframe::wgpu;
use neumann_control_app::app::NeumannApp;
use neumann_control_app::spawner::CoreSpawner;
use neumann_control_app::texts;
use neumann_control_core::settings::{self, WindowGeometry, LOG_FILE, SETTINGS_FILE};
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};

const WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
const WINDOW_MIN_SIZE: [f32; 2] = [480.0, 360.0];
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

/// The saved window, never smaller than the minimum; the default size when none was saved.
fn viewport(saved: Option<WindowGeometry>) -> ViewportBuilder {
    let builder = ViewportBuilder::default()
        .with_title(texts::WINDOW_TITLE)
        .with_min_inner_size(WINDOW_MIN_SIZE);
    let Some(saved) = saved else {
        return builder.with_inner_size(WINDOW_SIZE);
    };
    let width = saved.width.max(WINDOW_MIN_SIZE[0]);
    let height = saved.height.max(WINDOW_MIN_SIZE[1]);
    builder
        .with_position(pos2(saved.x, saved.y))
        .with_inner_size(vec2(width, height))
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
    start_log(directory);
    let settings_path = directory.join(SETTINGS_FILE);
    let saved_window = settings::load(&settings_path).window;
    let options = eframe::NativeOptions {
        viewport: viewport(saved_window),
        wgpu_options: wgpu_options(),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        texts::WINDOW_TITLE,
        options,
        Box::new(move |creation_context| {
            let context = creation_context.egui_ctx.clone();
            let spawner = Box::new(CoreSpawner::new(context.clone()));
            Ok(Box::new(NeumannApp::new(&context, settings_path, spawner)))
        }),
    )?;
    Ok(())
}
