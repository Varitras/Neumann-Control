//! The SSC paths the app handles by name; everything else comes from the catalog.

use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::ssc::wire::SscPath;

pub const OUTPUT_LEVEL: &str = "audio/out/level";
pub const MUTE: &str = "audio/out/mute";
pub const IDENTIFY: &str = "device/identification/visual";
pub const INPUT_METER: &str = "m/in/level";
pub const OUTPUT_METER: &str = "m/out/level";
pub const INPUT_CLIP: &str = "m/in/clip";

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
        ] {
            assert!(catalog_entry(catalog, &path(text)).is_some(), "{text}");
        }
    }
}
