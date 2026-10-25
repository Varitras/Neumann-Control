//! The display language the system shows its own interface in, for the language "like Windows"
//! (on Linux: like the system).

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

/// The locale variables in the order glibc asks them for messages; `LANGUAGE` is a list of
/// which the first counts.
#[cfg(target_os = "linux")]
const LOCALE_VARIABLES: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];
#[cfg(target_os = "linux")]
const GERMAN_LOCALE: &str = "de";

#[cfg(target_os = "linux")]
pub fn detect() -> Language {
    language_of_locale(|name| std::env::var(name).ok())
}

/// German when the first set locale variable names German ("de_DE.UTF-8", "de_AT:en"), English
/// for every other language and for none.
#[cfg(target_os = "linux")]
fn language_of_locale(variable: impl Fn(&str) -> Option<String>) -> Language {
    let first = LOCALE_VARIABLES
        .iter()
        .filter_map(|name| variable(name))
        .find(|value| !value.is_empty());
    let Some(value) = first else {
        return Language::English;
    };
    let language = value.split([':', '_', '.', '@']).next().unwrap_or_default();
    if language == GERMAN_LOCALE {
        return Language::German;
    }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn the_first_set_locale_variable_tells_the_language() {
        let set = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        assert_eq!(
            language_of_locale(set(&[("LANG", "de_DE.UTF-8")])),
            Language::German
        );
        assert_eq!(
            language_of_locale(set(&[("LANGUAGE", "de_AT:en"), ("LANG", "en_US.UTF-8")])),
            Language::German
        );
        assert_eq!(
            language_of_locale(set(&[
                ("LANGUAGE", ""),
                ("LC_ALL", "en_GB.UTF-8"),
                ("LANG", "de_DE")
            ])),
            Language::English
        );
        assert_eq!(
            language_of_locale(set(&[("LC_MESSAGES", "de")])),
            Language::German
        );
        assert_eq!(
            language_of_locale(set(&[("LANG", "C.UTF-8")])),
            Language::English
        );
        assert_eq!(language_of_locale(set(&[])), Language::English);
    }
}
