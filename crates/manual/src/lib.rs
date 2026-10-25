//! The manual shipped next to the exe: the README as one HTML file. Its pictures are embedded,
//! links to the repository's files point to GitHub (the release folder does not hold them), and
//! headings get the ids GitHub gives them, so the README's table of contents works.

use std::collections::BTreeSet;

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

/// Where links to files that are not shipped lead.
/// From the workspace manifest, as the app's help menu.
pub const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");
/// Files of the release folder: a link to them stays relative.
const SHIPPED: [&str; 1] = ["LICENSE"];
const TITLE: &str = "Neumann Control manual";
const STYLE: &str = "\
:root { color-scheme: light dark; --text: #1f2328; --page: #ffffff; --line: #d0d7de; --soft: #f6f8fa; }
@media (prefers-color-scheme: dark) {
  :root { --text: #e6edf3; --page: #0d1117; --line: #30363d; --soft: #161b22; }
}
body { margin: 0; background: var(--page); color: var(--text);
  font: 16px/1.6 \"Segoe UI\", system-ui, sans-serif; }
main { max-width: 52rem; margin: 0 auto; padding: 2rem 1rem 4rem; }
img { max-width: 100%; height: auto; }
h1, h2 { border-bottom: 1px solid var(--line); padding-bottom: .3em; }
code { background: var(--soft); padding: .1em .3em; border-radius: 4px; font-size: 90%; }
pre { background: var(--soft); padding: 1em; overflow-x: auto; border-radius: 6px; }
pre code { padding: 0; background: none; }
table { border-collapse: collapse; display: block; overflow-x: auto; }
th, td { border: 1px solid var(--line); padding: .4em .8em; vertical-align: top; }
a { color: #0969da; }
@media (prefers-color-scheme: dark) { a { color: #4493f8; } }
";

/// The manual for `markdown`; `read` gives the bytes of a file named relative to the README.
/// Fails on a picture that cannot be read or a link to a heading that does not exist, so a
/// broken manual stops the release instead of shipping.
pub fn render(markdown: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Result<String, String> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let ids = name_headings(&mut events);
    for event in &mut events {
        let Event::Start(tag) = event else {
            continue;
        };
        match tag {
            Tag::Image { dest_url, .. } => *dest_url = embedded(dest_url, &read)?.into(),
            Tag::Link { dest_url, .. } => *dest_url = link(dest_url, &ids)?.into(),
            _ => {}
        }
    }
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    Ok(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{TITLE}</title>\n<style>\n{STYLE}</style>\n</head>\n<body>\n<main>\n{body}</main>\n\
         </body>\n</html>\n"
    ))
}

/// Gives every heading the id GitHub gives it and returns them all.
fn name_headings(events: &mut [Event]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut index = 0;
    while index < events.len() {
        if !matches!(events[index], Event::Start(Tag::Heading { .. })) {
            index += 1;
            continue;
        }
        let mut text = String::new();
        let mut end = index + 1;
        while !matches!(events[end], Event::End(TagEnd::Heading(_))) {
            if let Event::Text(part) | Event::Code(part) = &events[end] {
                text.push_str(part);
            }
            end += 1;
        }
        let id = unique(slug(&text), &ids);
        if let Event::Start(Tag::Heading { id: heading, .. }) = &mut events[index] {
            *heading = Some(CowStr::from(id.clone()));
        }
        ids.insert(id);
        index = end + 1;
    }
    ids
}

/// GitHub's heading anchor: lower case, punctuation dropped, spaces as hyphens.
fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .map(|character| if character == ' ' { '-' } else { character })
        .collect()
}

/// A repeated heading gets "-1", "-2" … as on GitHub.
fn unique(slug: String, taken: &BTreeSet<String>) -> String {
    if !taken.contains(&slug) {
        return slug;
    }
    (1..)
        .map(|number| format!("{slug}-{number}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused number")
}

fn embedded(url: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Result<String, String> {
    if is_absolute(url) {
        return Ok(url.to_owned());
    }
    let media_type = match url.rsplit('.').next().map(str::to_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("gif") => "image/gif",
        _ => return Err(format!("{url}: not a picture the manual can embed")),
    };
    let bytes = read(url).ok_or_else(|| format!("{url}: cannot be read"))?;
    Ok(format!("data:{media_type};base64,{}", base64(&bytes)))
}

fn link(url: &str, ids: &BTreeSet<String>) -> Result<String, String> {
    if let Some(anchor) = url.strip_prefix('#') {
        return match ids.contains(anchor) {
            true => Ok(url.to_owned()),
            false => Err(format!("{url}: no heading has this anchor")),
        };
    }
    if is_absolute(url) || SHIPPED.contains(&url) {
        return Ok(url.to_owned());
    }
    Ok(format!("{REPOSITORY_URL}/blob/main/{url}"))
}

fn is_absolute(url: &str) -> bool {
    url.contains("://") || url.starts_with("mailto:") || url.starts_with("data:")
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let bits = u32::from_be_bytes([0, group[0], group[1], group[2]]);
        for position in 0..4 {
            let present = position <= chunk.len();
            let sextet = (bits >> (18 - 6 * position)) & 0x3f;
            text.push(match present {
                true => char::from(ALPHABET[sextet as usize]),
                false => '=',
            });
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn headings_get_githubs_anchors_and_the_contents_links_work() {
        let markdown = "## Contents\n\n- [Files and privacy](#files-and-privacy)\n\n\
                        ## Files and privacy\n\n## Files and privacy\n";
        let manual = render(markdown, no_files).unwrap();
        assert!(manual.contains("<h2 id=\"files-and-privacy\">"), "{manual}");
        assert!(
            manual.contains("<h2 id=\"files-and-privacy-1\">"),
            "{manual}"
        );
        assert!(manual.contains("href=\"#files-and-privacy\""), "{manual}");
    }

    #[test]
    fn a_link_to_a_missing_heading_fails() {
        let error = render("[Gone](#gone)\n", no_files).unwrap_err();
        assert!(error.contains("#gone"), "{error}");
    }

    #[test]
    fn slugs_drop_punctuation_like_github() {
        assert_eq!(slug("History and undo"), "history-and-undo");
        assert_eq!(slug("Faders & knobs (v2)"), "faders--knobs-v2");
        assert_eq!(slug("`settings.json`"), "settingsjson");
    }

    #[test]
    fn pictures_are_embedded() {
        let read = |name: &str| (name == "docs/desk.png").then(|| b"Man".to_vec());
        let manual = render("![Desk](docs/desk.png)\n", read).unwrap();
        assert!(
            manual.contains("src=\"data:image/png;base64,TWFu\""),
            "{manual}"
        );
        assert!(render("![Gone](docs/gone.png)\n", read).is_err());
    }

    #[test]
    fn repository_files_lead_to_github_and_shipped_ones_stay() {
        let manual = render(
            "[paths](crates/core/src/paths.toml) [MIT](LICENSE)\n",
            no_files,
        );
        let manual = manual.unwrap();
        let github = format!("href=\"{REPOSITORY_URL}/blob/main/crates/core/src/paths.toml\"");
        assert!(manual.contains(&github), "{manual}");
        assert!(manual.contains("href=\"LICENSE\""), "{manual}");
    }

    #[test]
    fn each_picture_gets_its_media_type() {
        let read = |_: &str| Some(b"Man".to_vec());
        for (name, media_type) in [
            ("a.jpg", "image/jpeg"),
            ("a.JPEG", "image/jpeg"),
            ("a.svg", "image/svg+xml"),
            ("a.gif", "image/gif"),
        ] {
            let manual = render(
                &format!(
                    "![x]({name})
"
                ),
                read,
            )
            .unwrap();
            assert!(
                manual.contains(&format!("data:{media_type};base64,")),
                "{name}"
            );
        }
        assert!(render(
            "![x](a.txt)
",
            read
        )
        .is_err());
    }

    #[test]
    fn web_addresses_stay_as_they_are() {
        let markdown = "[site](https://example.com/a) [mail](mailto:a@example.com)                         ![logo](https://example.com/logo.png)
";
        let manual = render(markdown, no_files).unwrap();
        assert!(
            manual.contains("href=\"https://example.com/a\""),
            "{manual}"
        );
        assert!(manual.contains("href=\"mailto:a@example.com\""), "{manual}");
        assert!(
            manual.contains("src=\"https://example.com/logo.png\""),
            "{manual}"
        );
    }

    #[test]
    fn tables_are_tables() {
        let manual = render(
            "| File | Content |
|---|---|
| a | b |
",
            no_files,
        )
        .unwrap();
        assert!(manual.contains("<table>"), "{manual}");
    }

    #[test]
    fn base64_pads_as_the_standard_says() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Many"), "TWFueQ==");
    }

    /// The real README: every anchor of its contents and every picture resolves.
    #[test]
    fn the_readme_renders() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
        let markdown = std::fs::read_to_string(format!("{root}README.md")).unwrap();
        let read = |name: &str| std::fs::read(format!("{root}{name}")).ok();
        let manual = render(&markdown, read).unwrap();
        assert!(
            manual.contains("<h3 id=\"compact-mode\">"),
            "a heading of the README"
        );
    }
}
