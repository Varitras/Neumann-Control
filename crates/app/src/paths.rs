//! The SSC paths the app handles by name; everything else comes from the catalog.

use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::ssc::wire::SscPath;

pub const OUTPUT_LEVEL: &str = "audio/out/level";
pub const MUTE: &str = "audio/out/mute";
pub const PHASE_INVERSION: &str = "audio/out/phaseinversion";
pub const IDENTIFY: &str = "device/identification/visual";
pub const INPUT_METER: &str = "m/in/level";
pub const OUTPUT_METER: &str = "m/out/level";
pub const INPUT_CLIP: &str = "m/in/clip";
pub const OUTPUT_CLIP: &str = "m/out/clip";
pub const PRODUCT: &str = "device/identity/product";
pub const FIRMWARE_VERSION: &str = "device/identity/version";
pub const SERIAL: &str = "device/identity/serial";
pub const MAC_ADDRESSES: &str = "device/network/ether/macs";
pub const WARNINGS: &str = "warnings";
pub const DEVICE_NAME: &str = "device/name";
pub const TEMPERATURE: &str = "device/temperature";
pub const CONTROL_MODE: &str = "ui/control_mode";
pub const INPUT_SELECT: &str = "ui/input_select";
pub const STANDBY_ENABLED: &str = "device/standby/enabled";
pub const INPUT_INTERFACE: &str = "audio/in/interface";
pub const DELAY: &str = "audio/out/delay";
pub const STANDBY_TIME: &str = "device/standby/auto_standby_time";
pub const STANDBY_COUNTDOWN: &str = "device/standby/countdown";
pub const CURRENT_INPUT: &str = "audio/in/current_input";
pub const LOGO_BRIGHTNESS: &str = "ui/logo/brightness";
pub const MUTE_OUTPUT_A: &str = "audio/out1/mute";
pub const MUTE_OUTPUT_B: &str = "audio/out2/mute";

pub fn path(text: &str) -> SscPath {
    SscPath::parse(text).expect("constant path is valid")
}

pub fn catalog_entry<'a>(catalog: &'a Catalog, path: &SscPath) -> Option<&'a Entry> {
    catalog.entries.iter().find(|entry| entry.path == *path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_path_is_in_the_catalog() {
        let catalog = Catalog::embedded();
        for text in [
            OUTPUT_LEVEL,
            MUTE,
            IDENTIFY,
            INPUT_METER,
            OUTPUT_METER,
            INPUT_CLIP,
            OUTPUT_CLIP,
            PRODUCT,
            FIRMWARE_VERSION,
            SERIAL,
            WARNINGS,
            CONTROL_MODE,
            INPUT_SELECT,
            STANDBY_ENABLED,
            INPUT_INTERFACE,
        ] {
            assert!(catalog_entry(catalog, &path(text)).is_some(), "{text}");
        }
    }
}
