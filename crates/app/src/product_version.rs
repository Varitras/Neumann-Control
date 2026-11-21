// The product version in `VERSION`, checked and packed for the Windows executable. `build.rs`
// includes this file (a build script has no crate to import from), and the library compiles it
// for its tests; plain comments, as an included file takes no inner doc comment.

/// The file's text as `major.minor.patch`, or why it is not one.
pub fn checked_version(text: &str) -> Result<String, String> {
    let version = text.trim();
    // Each part is one 16-bit field of the exe's version.
    let well_formed = version.split('.').count() == 3
        && version.split('.').all(|part| {
            part.chars().all(|digit| digit.is_ascii_digit()) && part.parse::<u16>().is_ok()
        });
    match well_formed {
        true => Ok(version.to_owned()),
        false => Err(format!(
            "one version like 0.2.1, each part at most {}, found {version:?}",
            u16::MAX
        )),
    }
}

/// Windows' four 16-bit fields, major first; the fourth stays 0.
pub fn numeric_version(version: &str) -> u64 {
    version
        .split('.')
        .map(|part| part.parse::<u64>().expect("checked digits"))
        .zip([48, 32, 16])
        .fold(0, |packed, (part, shift)| packed | (part << shift))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_version_is_taken_and_packed() {
        assert_eq!(checked_version("0.2.1\r\n"), Ok("0.2.1".to_owned()));
        assert_eq!(numeric_version("1.2.3"), (1 << 48) | (2 << 32) | (3 << 16));
    }

    /// No suffix (`-alpha.1`): the release workflow and the exe's version take numbers only.
    #[test]
    fn a_version_with_a_suffix_or_without_three_parts_is_refused() {
        for text in ["0.3.0-alpha.1", "0.2", "0.2.1.4", "v0.2.1", ""] {
            assert!(checked_version(text).is_err(), "{text:?}");
        }
    }

    /// Each part is one 16-bit field of the exe's version; a bigger one spilled into the next
    /// field, and one too big for a number stopped the build with "checked digits".
    #[test]
    fn a_part_beyond_a_windows_version_field_is_refused() {
        for text in ["0.65536.0", "1.0.99999999999999999999"] {
            assert!(checked_version(text).is_err(), "{text:?}");
        }
        assert!(checked_version("65535.65535.65535").is_ok());
    }
}
