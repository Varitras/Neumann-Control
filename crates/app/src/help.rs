//! What the help menu reaches outside the app: the manual next to the exe, the web page of a
//! release, and the question whether GitHub has a newer release. Only asked when the user clicks;
//! the app has no updater. Behind a trait, so tests neither start a browser nor go online.

use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

/// Written next to the exe by the release build (build-release.ps1 names it the same).
pub const MANUAL_FILE: &str = "neumann-control-manual.html";
pub const REPOSITORY_URL: &str = "https://github.com/Varitras/Neumann-Control";
pub const RELEASES_URL: &str = "https://github.com/Varitras/Neumann-Control/releases";
/// The SSC protocol reference and the speakers' measured behaviour.
pub const WIKI_URL: &str = "https://github.com/Varitras/Neumann-Control/wiki";
/// GitHub's API for the newest release that is not a draft or a prerelease.
pub const API_HOST: &str = "api.github.com";
pub const LATEST_RELEASE_PATH: &str = "/repos/Varitras/Neumann-Control/releases/latest";

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
    fn latest_release(&self) -> Result<Option<Release>, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateResult {
    Newer(Release),
    Current,
    NoRelease,
    /// No answer, or one that could not be read; the reason is logged.
    Failed,
}

/// What the answer means for the running version `current`.
pub fn verdict(current: &str, latest: Result<Option<Release>, String>) -> UpdateResult {
    let release = match latest {
        Ok(Some(release)) => release,
        Ok(None) => return UpdateResult::NoRelease,
        Err(error) => {
            log::warn!("update check failed: {error}");
            return UpdateResult::Failed;
        }
    };
    let (Some(published), Some(running)) = (version(&release.tag), version(current)) else {
        log::warn!(
            "update check: cannot compare {:?} with {current}",
            release.tag
        );
        return UpdateResult::Failed;
    };
    match published > running {
        true => UpdateResult::Newer(release),
        false => UpdateResult::Current,
    }
}

/// "v1.2.3" or "1.2.3", a suffix such as "-beta" ignored.
fn version(text: &str) -> Option<[u64; 3]> {
    let core = text.strip_prefix('v').unwrap_or(text);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(version)
}

/// GitHub's answer for the latest release. A page outside this repository is replaced by the
/// list of releases: the app opens it in the browser.
pub fn parse_release(body: &str) -> Result<Release, String> {
    let json: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("the answer has no tag_name")?;
    let page = json.get("html_url").and_then(Value::as_str);
    let own = page.filter(|url| url.starts_with(&format!("{REPOSITORY_URL}/")));
    Ok(Release {
        tag: tag.to_owned(),
        url: own.unwrap_or(RELEASES_URL).to_owned(),
    })
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

    use super::{parse_release, Help, Release, API_HOST, LATEST_RELEASE_PATH};

    /// GitHub refuses API calls without one.
    const USER_AGENT: &str = concat!("neumann-control/", env!("CARGO_PKG_VERSION"));
    const HTTPS_PORT: u16 = 443;
    /// Per step (name lookup, connect, send, receive), in milliseconds: the dialog waits on it.
    const TIMEOUT_MS: i32 = 10_000;
    const STATUS_OK: u32 = 200;
    /// The API's answer while a repository has no release.
    const STATUS_NOT_FOUND: u32 = 404;
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

        fn latest_release(&self) -> Result<Option<Release>, String> {
            let (status, body) = get(API_HOST, LATEST_RELEASE_PATH)?;
            match status {
                STATUS_OK => parse_release(&body).map(Some),
                STATUS_NOT_FOUND => Ok(None),
                other => Err(format!("GitHub answered {other}")),
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
    fn no_release_and_failures_are_told_apart() {
        assert_eq!(verdict("0.1.0", Ok(None)), UpdateResult::NoRelease);
        assert_eq!(
            verdict("0.1.0", Err("offline".to_owned())),
            UpdateResult::Failed
        );
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("latest")))),
            UpdateResult::Failed
        );
        assert_eq!(
            verdict("0.1.0", Ok(Some(release("v1.2")))),
            UpdateResult::Failed
        );
    }

    #[test]
    fn the_answer_gives_the_tag_and_only_a_page_of_this_repository() {
        let own = format!(
            r#"{{"tag_name": "v0.2.0", "html_url": "{REPOSITORY_URL}/releases/tag/v0.2.0"}}"#
        );
        let parsed = parse_release(&own).unwrap();
        assert_eq!(parsed.tag, "v0.2.0");
        assert_eq!(parsed.url, format!("{REPOSITORY_URL}/releases/tag/v0.2.0"));
        let foreign = r#"{"tag_name": "v0.2.0", "html_url": "https://example.com/x"}"#;
        assert_eq!(parse_release(foreign).unwrap().url, RELEASES_URL);
        assert!(parse_release(r#"{"name": "x"}"#).is_err());
        assert!(parse_release("not json").is_err());
    }
}

/// Goes online, so only on request: `cargo test -p neumann-control-app --lib -- --ignored github`.
#[cfg(all(test, windows))]
mod online {
    use super::*;

    #[test]
    #[ignore = "asks GitHub"]
    fn github_answers_the_latest_release_question() {
        let answer = WindowsHelp.latest_release();
        assert!(answer.is_ok(), "{answer:?}");
        println!("{answer:?}");
    }
}
