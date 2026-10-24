//! The EQ graph's colours: one per band number and one per kind of container, each the
//! default unless the user chose another in the settings.

use eframe::egui::Color32;
use neumann_control_core::settings::EqDisplaySettings;
use neumann_control_core::ssc::wire::SscPath;

use super::theme::group_color;

/// The categorical order of the reference palette, validated for neighbouring pairs in both
/// themes. A band beyond the eighth repeats it: the number in its point names the band, the
/// colour only helps.
const BANDS_LIGHT: [Color32; 8] = [
    Color32::from_rgb(0x2a, 0x78, 0xd6),
    Color32::from_rgb(0xeb, 0x68, 0x34),
    Color32::from_rgb(0x1b, 0xaf, 0x7a),
    Color32::from_rgb(0xed, 0xa1, 0x00),
    Color32::from_rgb(0xe8, 0x7b, 0xa4),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x62, 0x50, 0xd6),
    Color32::from_rgb(0xe3, 0x49, 0x48),
];
const BANDS_DARK: [Color32; 8] = [
    Color32::from_rgb(0x39, 0x87, 0xe5),
    Color32::from_rgb(0xd9, 0x59, 0x26),
    Color32::from_rgb(0x19, 0x9e, 0x70),
    Color32::from_rgb(0xc9, 0x85, 0x00),
    Color32::from_rgb(0xd5, 0x51, 0x81),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x90, 0x85, 0xe9),
    Color32::from_rgb(0xe6, 0x67, 0x67),
];

/// The containers below an output, in the order their curves had their colours so far.
pub const CURVE_KEYS: [&str; 4] = ["eq2", "eq3", "eq1/in1", "eq1/in2"];
/// The most bands a container has (KH 120 II `eq3`); the settings offer this many colours.
pub const BAND_COLOR_COUNT: usize = 20;

/// Text on a point of `color`: dark ink on a light fill, light ink on a dark one.
const DARK_INK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x19);
const LIGHT_INK: Color32 = Color32::from_rgb(0xf0, 0xef, 0xec);
/// Relative luminance above which dark ink reads better than light ink.
const LIGHT_FILL_LUMINANCE: f32 = 0.18;

pub fn default_band_color(band: usize, dark: bool) -> Color32 {
    let palette = if dark { &BANDS_DARK } else { &BANDS_LIGHT };
    palette[band % palette.len()]
}

/// `band` counts from 0; the settings count from 1, as the user does.
pub fn band_color(display: &EqDisplaySettings, band: usize, dark: bool) -> Color32 {
    display
        .band_colors
        .get(&band_key(band))
        .and_then(|text| parse_hex(text))
        .unwrap_or_else(|| default_band_color(band, dark))
}

/// `eq2`, `eq3`, `eq1/in1` from `audio/out1/eq1/in1`: the container below its output.
/// The band's key in the settings' colours and its number for the user: counted from 1.
pub fn band_key(band: usize) -> String {
    (band + 1).to_string()
}

pub fn curve_key(container: &SscPath) -> String {
    container.segments().get(2..).unwrap_or_default().join("/")
}

pub fn default_curve_color(key: &str) -> Color32 {
    let place = CURVE_KEYS
        .iter()
        .position(|known| *known == key)
        .unwrap_or_default();
    group_color(place as u8)
}

pub fn curve_color(display: &EqDisplaySettings, key: &str) -> Color32 {
    display
        .curve_colors
        .get(key)
        .and_then(|text| parse_hex(text))
        .unwrap_or_else(|| default_curve_color(key))
}

/// `#rrggbb`; anything else is not a colour.
pub fn parse_hex(text: &str) -> Option<Color32> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

pub fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

pub fn ink_on(color: Color32) -> Color32 {
    let linear = |channel: u8| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance =
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b());
    if luminance > LIGHT_FILL_LUMINANCE {
        DARK_INK
    } else {
        LIGHT_INK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_take_the_palette_in_order_and_repeat_after_eight() {
        assert_eq!(default_band_color(0, true), BANDS_DARK[0]);
        assert_eq!(default_band_color(7, false), BANDS_LIGHT[7]);
        assert_eq!(default_band_color(8, true), default_band_color(0, true));
    }

    #[test]
    fn a_chosen_colour_wins_and_nonsense_falls_back() {
        let mut display = EqDisplaySettings::default();
        display
            .band_colors
            .insert("3".to_owned(), "#ff0000".to_owned());
        display.band_colors.insert("4".to_owned(), "red".to_owned());
        assert_eq!(band_color(&display, 2, true), Color32::from_rgb(255, 0, 0));
        assert_eq!(band_color(&display, 3, true), default_band_color(3, true));
        display
            .curve_colors
            .insert("eq3".to_owned(), "#00ff00".to_owned());
        assert_eq!(curve_color(&display, "eq3"), Color32::from_rgb(0, 255, 0));
        assert_eq!(curve_color(&display, "eq2"), group_color(0));
    }

    #[test]
    fn hex_reads_back() {
        let color = Color32::from_rgb(0x2a, 0x78, 0xd6);
        assert_eq!(parse_hex(&hex(color)), Some(color));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#gg0000"), None);
    }

    #[test]
    fn the_curve_key_is_the_container_below_its_output() {
        let key = |text: &str| curve_key(&SscPath::parse(text).unwrap());
        assert_eq!(key("audio/out/eq2"), "eq2");
        assert_eq!(key("audio/out1/eq1/in2"), "eq1/in2");
        assert_eq!(default_curve_color("eq1/in1"), group_color(2));
    }

    #[test]
    fn the_number_in_a_point_has_contrast() {
        assert_eq!(ink_on(Color32::from_rgb(0xed, 0xa1, 0x00)), DARK_INK);
        assert_eq!(ink_on(Color32::from_rgb(0x00, 0x83, 0x00)), LIGHT_INK);
        assert_eq!(ink_on(Color32::from_rgb(0x62, 0x50, 0xd6)), LIGHT_INK);
    }
}
