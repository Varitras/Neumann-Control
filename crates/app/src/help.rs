//! What the help menu reaches outside the app: the manual next to the exe, the web page of a
//! release, and the question whether GitHub has a newer release. Only asked when the user clicks;
//! the app has no updater. Behind a trait, so tests neither start a browser nor go online.

use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

/// Written next to the exe by the release build (build-release.ps1 names it the same, which a
/// source guard test checks).
pub const MANUAL_FILE: &str = "neumann-control-manual.html";
/// From the workspace manifest, as the manual's links to repository files.
pub const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");
pub const RELEASES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");
/// The SSC protocol reference and the speakers' measured behaviour.
pub const WIKI_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/wiki");
/// GitHub's API, asked for this repository's releases.
pub const API_HOST: &str = "api.github.com";
const GITHUB_PAGES: &str = "https://github.com/";
/// The newest releases GitHub lists first; an update lies among them.
const RELEASES_ASKED: usize = 10;

/// `/repos/<owner>/<name>/releases` of this repository. Not `/releases/latest`: that leaves
/// pre-releases out and answered 404 while the only release was the 0.1.17 pre-release.
pub fn releases_path() -> String {
    let repository = REPOSITORY_URL.trim_start_matches(GITHUB_PAGES);
    format!("/repos/{repository}/releases?per_page={RELEASES_ASKED}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The tag as published, e.g. "v0.2.0".
    pub tag: String,
    /// Its page; only ever one of this repository's.
    pub url: String,
}

pub trait Help: Send + Sync {
    /// Opens a file or web address as Explorer would; false when Windows could not.
    fn open(&self, target: &str) -> bool;
    /// The newest release; `Ok(None)` while none is published.
    fn latest_release(&self) -> Result<Option<Release>, LookupError>;
}

/// Why the newest release could not be told; the text is for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// No answer from GitHub (offline, proxy, timeout).
    Unreachable(String),
    /// GitHub answered, but not with a release the app can read (a rate limit, another form).
    Unusable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateResult {
    Newer(Release),
    Current,
    NoRelease,
    /// No answer from GitHub; the reason is logged.
    Unreachable,
    /// An answer that could not be read, or a tag that is no version; the reason is logged.
    Unusable,
}

/// What the answer means for the running version `current`.
pub fn verdict(current: &str, latest: Result<Option<Release>, LookupError>) -> UpdateResult {
    let release = match latest {
        Ok(Some(release)) => release,
        Ok(None) => return UpdateResult::NoRelease,
        Err(LookupError::Unreachable(reason)) => {
            log::warn!("update check: GitHub not reached: {reason}");
            return UpdateResult::Unreachable;
        }
        Err(LookupError::Unusable(reason)) => {
            log::warn!("update check: answer not usable: {reason}");
            return UpdateResult::Unusable;
        }
    };
    let (Some(published), Some(running)) = (version(&release.tag), version(current)) else {
        log::warn!(
            "update check: cannot compare {:?} with {current}",
            release.tag
        );
        return UpdateResult::Unusable;
    };
    match published > running {
        true => UpdateResult::Newer(release),
        false => UpdateResult::Current,
    }
}

/// "v1.2.3" or "1.2.3", and whether it is final: a pre-release ("-beta.1") comes before its
/// final version. Two pre-releases of one version count as the same.
fn version(text: &str) -> Option<([u64; 3], bool)> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let text = text.split('+').next()?;
    let core = text.split('-').next()?;
    let is_final = core.len() == text.len();
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((version, is_final))
}

/// One release of GitHub's list. A page outside this repository is replaced by the list of
/// releases: the app opens it in the browser.
fn release_of(json: &Value) -> Result<Release, String> {
    let tag = json
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("a release has no tag_name")?;
    let page = json.get("html_url").and_then(Value::as_str);
    let own = page.filter(|url| url.starts_with(&format!("{REPOSITORY_URL}/")));
    Ok(Release {
        tag: tag.to_owned(),
        url: own.unwrap_or(RELEASES_URL).to_owned(),
    })
}

/// The highest published version in GitHub's list of this repository's releases, pre-releases
/// included; `None` while the list is empty. Drafts do not count.
pub fn parse_releases(body: &str) -> Result<Option<Release>, String> {
    let json: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let entries = json.as_array().ok_or("the answer is no list of releases")?;
    let published = entries
        .iter()
        .filter(|entry| !entry.get("draft").and_then(Value::as_bool).unwrap_or(false));
    let mut releases = Vec::new();
    for entry in published {
        releases.push(release_of(entry)?);
    }
    if releases.is_empty() {
        return Ok(None);
    }
    releases
        .into_iter()
        .filter_map(|release| version(&release.tag).map(|number| (number, release)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| Some(release))
        .ok_or_else(|| "no release has a version as its tag".to_owned())
}

/// A check running on its own thread; the window is woken when it has an answer.
pub struct UpdateCheck {
    result: Arc<Mutex<Option<UpdateResult>>>,
}

impl UpdateCheck {
    pub fn start(help: Arc<dyn Help>, wake: Arc<dyn Fn() + Send + Sync>) -> UpdateCheck {
        let result = Arc::new(Mutex::new(None));
        let answer = Arc::clone(&result);
        thread::spawn(move || {
            let found = verdict(env!("CARGO_PKG_VERSION"), help.latest_release());
            *answer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(found);
            wake();
        });
        UpdateCheck { result }
    }

    /// `None` while the answer is still out.
    pub fn result(&self) -> Option<UpdateResult> {
        self.result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[cfg(windows)]
pub use windows_help::WindowsHelp;

#[cfg(windows)]
mod windows_help {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::{parse_releases, releases_path, Help, LookupError, Release, API_HOST};

    /// GitHub refuses API calls without one.
    const USER_AGENT: &str = concat!("neumann-control/", env!("CARGO_PKG_VERSION"));
    const HTTPS_PORT: u16 = 443;
    /// Per step (name lookup, connect, send, receive), in milliseconds: the dialog waits on it.
    const TIMEOUT_MS: i32 = 10_000;
    const STATUS_OK: u32 = 200;
    const READ_CHUNK: usize = 16 * 1024;
    /// ShellExecute's success is any value above 32.
    const SHELL_ERROR_LIMIT: isize = 32;

    pub struct WindowsHelp;

    impl Help for WindowsHelp {
        fn open(&self, target: &str) -> bool {
            let operation = wide("open");
            let file = wide(target);
            // SAFETY: both strings are NUL-terminated and outlive the call.
            let result = unsafe {
                ShellExecuteW(
                    null_mut(),
                    operation.as_ptr(),
                    file.as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                )
            };
            result as isize > SHELL_ERROR_LIMIT
        }

        fn latest_release(&self) -> Result<Option<Release>, LookupError> {
            let (status, body) =
                get(API_HOST, &releases_path()).map_err(LookupError::Unreachable)?;
            // A repository without releases lists none; anything but 200 is not an answer.
            match status {
                STATUS_OK => parse_releases(&body).map_err(LookupError::Unusable),
                other => Err(LookupError::Unusable(format!("GitHub answered {other}"))),
            }
        }
    }

    /// Closes a WinHTTP handle when dropped.
    struct Handle(*mut c_void);

    impl Handle {
        fn new(raw: *mut c_void, what: &str) -> Result<Handle, String> {
            match raw.is_null() {
                true => Err(format!("{what}: {}", std::io::Error::last_os_error())),
                false => Ok(Handle(raw)),
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from WinHTTP and is closed once.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }

    fn check(succeeded: i32, what: &str) -> Result<(), String> {
        match succeeded {
            0 => Err(format!("{what}: {}", std::io::Error::last_os_error())),
            _ => Ok(()),
        }
    }

    /// An HTTPS GET with Windows' proxy settings: the status and the body.
    fn get(host: &str, path: &str) -> Result<(u32, String), String> {
        let agent = wide(USER_AGENT);
        let host = wide(host);
        let verb = wide("GET");
        let path = wide(path);
        // SAFETY: every string is NUL-terminated and outlives the calls; every handle is checked
        // before use and closed by `Handle`, the request before the connection and the session.
        unsafe {
            let session = WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            );
            let session = Handle::new(session, "WinHttpOpen")?;
            let timeouts =
                WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
            check(timeouts, "WinHttpSetTimeouts")?;
            let connection = WinHttpConnect(session.0, host.as_ptr(), HTTPS_PORT, 0);
            let connection = Handle::new(connection, "WinHttpConnect")?;
            let request = WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            );
            let request = Handle::new(request, "WinHttpOpenRequest")?;
            check(
                WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0),
                "WinHttpSendRequest",
            )?;
            check(
                WinHttpReceiveResponse(request.0, null_mut()),
                "WinHttpReceiveResponse",
            )?;
            let mut status: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            check(
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    null(),
                    (&mut status as *mut u32).cast(),
                    &mut size,
                    null_mut(),
                ),
                "WinHttpQueryHeaders",
            )?;
            let mut body = Vec::new();
            let mut chunk = vec![0u8; READ_CHUNK];
            loop {
                let mut read: u32 = 0;
                check(
                    WinHttpReadData(
                        request.0,
                        chunk.as_mut_ptr().cast(),
                        chunk.len() as u32,
                        &mut read,
                    ),
                    "WinHttpReadData",
                )?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
            }
            Ok((status, String::from_utf8_lossy(&body).into_owned()))
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_owned(),
            url: RELEASES_URL.to_owned(),
        }
    }

    #[test]
    fn a_higher_published_version_is_newer() {
        let newer = verdict("0.1.0", Ok(Some(release("v0.2.0"))));
        assert_eq!(newer, UpdateResult::Newer(release("v0.2.0")));
        assert_eq!(
            verdict("0.10.0", Ok(Some(release("v0.9.9")))),
            UpdateResult::Current
        );
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("0.1.0")))),
            UpdateResult::Current
        );
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("v1.0.0-beta")))),
            UpdateResult::Newer(release("v1.0.0-beta"))
        );
    }

    #[test]
    fn the_api_asks_for_this_repositorys_releases() {
        assert_eq!(
            releases_path(),
            "/repos/Varitras/Neumann-Control/releases?per_page=10"
        );
        assert_eq!(WIKI_URL, "https://github.com/Varitras/Neumann-Control/wiki");
    }

    /// The final release is newer than its own beta, as in SemVer; build metadata is no beta.
    #[test]
    fn a_running_beta_is_older_than_its_final_release() {
        assert_eq!(
            verdict("1.0.0-beta.1", Ok(Some(release("v1.0.0")))),
            UpdateResult::Newer(release("v1.0.0"))
        );
        assert_eq!(
            verdict("1.0.0", Ok(Some(release("v1.0.0-beta.1")))),
            UpdateResult::Current
        );
        assert_eq!(
            verdict("1.0.0+build.7", Ok(Some(release("v1.0.0")))),
            UpdateResult::Current
        );
    }

    /// "Not reachable" only when GitHub did not answer: a rate limit or an unreadable answer is
    /// told apart, so nobody looks for a network fault that is not there.
    #[test]
    fn no_release_no_answer_and_an_unusable_answer_are_told_apart() {
        assert_eq!(verdict("0.1.0", Ok(None)), UpdateResult::NoRelease);
        let offline = LookupError::Unreachable("offline".to_owned());
        assert_eq!(verdict("0.1.0", Err(offline)), UpdateResult::Unreachable);
        let limited = LookupError::Unusable("GitHub answered 403".to_owned());
        assert_eq!(verdict("0.1.0", Err(limited)), UpdateResult::Unusable);
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("latest")))),
            UpdateResult::Unusable
        );
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("v1.2")))),
            UpdateResult::Unusable
        );
    }

    fn listed(tag: &str, prerelease: bool, draft: bool) -> String {
        format!(
            r#"{{"tag_name": "{tag}", "html_url": "{REPOSITORY_URL}/releases/tag/{tag}", "prerelease": {prerelease}, "draft": {draft}}}"#
        )
    }

    /// GitHub's "latest release" leaves pre-releases out and answered 404 while 0.1.17 was the
    /// only, pre-release, version: the check said "no release yet" and would never have told
    /// of a newer pre-release. The list counts them.
    #[test]
    fn the_newest_release_counts_pre_releases_too() {
        let body = format!(
            "[{}, {}]",
            listed("v0.1.18", true, false),
            listed("v0.1.17", true, false)
        );
        let newest = parse_releases(&body).unwrap().unwrap();
        assert_eq!(newest.tag, "v0.1.18");
        assert_eq!(newest.url, format!("{REPOSITORY_URL}/releases/tag/v0.1.18"));
    }

    /// The highest version counts, not the newest entry: a later fix of an older line is no
    /// update. A draft is not published.
    #[test]
    fn the_highest_published_version_counts() {
        let body = format!(
            "[{}, {}, {}]",
            listed("v0.3.0", false, true),
            listed("v0.1.20", false, false),
            listed("v0.2.0", true, false)
        );
        assert_eq!(parse_releases(&body).unwrap().unwrap().tag, "v0.2.0");
    }

    #[test]
    fn an_empty_list_is_no_release_and_a_list_without_versions_is_unusable() {
        assert_eq!(parse_releases("[]"), Ok(None));
        let body = format!("[{}]", listed("nightly", true, false));
        assert!(parse_releases(&body).is_err());
        assert!(parse_releases(r#"{"tag_name": "v1.0.0"}"#).is_err());
        assert!(parse_releases("not json").is_err());
    }

    #[test]
    fn the_answer_gives_the_tag_and_only_a_page_of_this_repository() {
        let own = format!(
            r#"[{{"tag_name": "v0.2.0", "html_url": "{REPOSITORY_URL}/releases/tag/v0.2.0"}}]"#
        );
        let parsed = parse_releases(&own).unwrap().unwrap();
        assert_eq!(parsed.tag, "v0.2.0");
        assert_eq!(parsed.url, format!("{REPOSITORY_URL}/releases/tag/v0.2.0"));
        let foreign = r#"[{"tag_name": "v0.2.0", "html_url": "https://example.com/x"}]"#;
        assert_eq!(parse_releases(foreign).unwrap().unwrap().url, RELEASES_URL);
        assert!(parse_releases(r#"[{"name": "x"}]"#).is_err());
    }
}

/// Goes online, so only on request: `cargo test -p neumann-control-app --lib -- --ignored github`.
#[cfg(all(test, windows))]
mod online {
    use super::*;

    #[test]
    #[ignore = "asks GitHub"]
    fn github_answers_the_newest_release_question() {
        let answer = WindowsHelp.latest_release();
        assert!(answer.is_ok(), "{answer:?}");
        println!("{answer:?}");
    }
}
