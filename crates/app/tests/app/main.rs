//! All window tests in one binary: each test binary linked the whole egui/wgpu stack anew,
//! which was most of a mutation run's build time.

mod support;

mod compact;
mod details;
mod eq;
mod glyphs;
mod groups;
mod help;
mod history;
mod speakers;
mod strip_layout;
mod system_audio;
mod views;
mod write_gestures;
mod write_messages;
