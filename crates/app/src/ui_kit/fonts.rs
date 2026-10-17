//! Inter for all text, egui's default fonts behind it for symbols and emoji.

use std::sync::Arc;

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily, FontId};

const FONT_INTER: &str = "Inter";
const FONT_INTER_SEMIBOLD: &str = "Inter-SemiBold";

fn font_data(bytes: &'static [u8]) -> Arc<FontData> {
    Arc::new(FontData::from_static(bytes))
}

pub fn definitions() -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    definitions.font_data.insert(
        FONT_INTER.to_owned(),
        font_data(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
    );
    definitions.font_data.insert(
        FONT_INTER_SEMIBOLD.to_owned(),
        font_data(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")),
    );
    let default_fallbacks = definitions.families[&FontFamily::Proportional].clone();
    let semibold_family = std::iter::once(FONT_INTER_SEMIBOLD.to_owned())
        .chain(default_fallbacks)
        .collect();
    definitions.families.insert(
        FontFamily::Name(FONT_INTER_SEMIBOLD.into()),
        semibold_family,
    );
    if let Some(names) = definitions.families.get_mut(&FontFamily::Proportional) {
        names.insert(0, FONT_INTER.to_owned());
    }
    definitions
}

/// Takes effect from the next frame on.
pub fn install(context: &Context) {
    context.set_fonts(definitions());
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_INTER_SEMIBOLD.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_comes_first_and_egui_defaults_stay_as_fallbacks() {
        let definitions = definitions();
        let proportional = &definitions.families[&FontFamily::Proportional];
        assert_eq!(proportional[0], FONT_INTER);
        assert!(proportional.len() > 1);
        let semibold = &definitions.families[&semibold(12.0).family];
        assert_eq!(semibold[0], FONT_INTER_SEMIBOLD);
        assert!(semibold.len() > 1);
    }

    /// cargo about only knows crates, so the release's THIRD-PARTY-LICENSES.html carries the
    /// embedded font's licence solely through its copy in the root `about.hbs`.
    #[test]
    fn the_licence_template_carries_every_embedded_font_licence_verbatim() {
        let manifest_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let template = std::fs::read_to_string(manifest_directory.join("../../about.hbs"))
            .expect("the licence template at the repo root");
        let fonts_directory = manifest_directory.join("assets/fonts");
        let mut licence_files = 0;
        for entry in std::fs::read_dir(&fonts_directory).expect("the fonts folder") {
            let name = entry.expect("a fonts folder entry").file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("OFL-") && name.ends_with(".txt")) {
                continue;
            }
            licence_files += 1;
            let text = std::fs::read_to_string(fonts_directory.join(&*name)).expect("the licence");
            let opening = format!("data-file=\"{name}\">");
            let start = template
                .find(&opening)
                .unwrap_or_else(|| panic!("{name} is missing from about.hbs"))
                + opening.len();
            let end = start + template[start..].find("</pre>").expect("the closing tag");
            assert_eq!(&template[start..end], text, "{name} differs in about.hbs");
        }
        assert!(licence_files > 0);
        assert_eq!(template.matches("data-file=\"OFL-").count(), licence_files);
    }
}
