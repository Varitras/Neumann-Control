//! The display language Windows shows its own interface in, for the language "like Windows".

use neumann_control_core::settings::Language;

/// The primary language part of a Windows language id (`LANG_GERMAN`), whatever the country.
const PRIMARY_LANGUAGE_MASK: u16 = 0x03ff;
const PRIMARY_GERMAN: u16 = 0x07;

/// German for German in any country, English for every other language.
pub fn language_of(language_id: u16) -> Language {
    if language_id & PRIMARY_LANGUAGE_MASK == PRIMARY_GERMAN {
        return Language::German;
    }
    Language::English
}

#[cfg(windows)]
pub fn detect() -> Language {
    // SAFETY: takes no arguments and only reads the user's setting.
    let language_id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    language_of(language_id)
}

#[cfg(not(windows))]
pub fn detect() -> Language {
    Language::English
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_in_any_country_is_german_and_the_rest_english() {
        assert_eq!(language_of(0x0407), Language::German);
        assert_eq!(language_of(0x0807), Language::German);
        assert_eq!(language_of(0x0c07), Language::German);
        assert_eq!(language_of(0x0409), Language::English);
        assert_eq!(language_of(0x040c), Language::English);
    }
}
