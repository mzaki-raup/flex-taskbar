//! Updating in place: asking the forge whether a newer release has been
//! published, and installing it.
//!
//! Everything slow happens on a helper thread (WinHTTP, hashing); the result
//! comes back to the UI thread as [`WM_APP_UPDATE`](super::app::WM_APP_UPDATE)
//! so dialogs and the swap happen where every other window does.
//!
//! What the user sees: *Check for updates…* in the menu looks now and says
//! what it finds; by itself the app looks once a day and only speaks up when
//! there is something newer. Either way the release is offered as a choice
//! between installing it, leaving it for now, and skipping that release for
//! good (remembered as `skipped_update`, so only something newer asks again).
//!
//! Installing is deliberately dull:
//!
//! 1. the program is downloaded to `data\update\`;
//! 2. its SHA-256 must match the `SHA256SUMS.txt` published beside it — a
//!    release without one is never installed automatically, only opened in
//!    the browser;
//! 3. the running `FlexTaskbar.exe` is **renamed** aside (Windows won't let a
//!    running image be overwritten, but it will let it be renamed) and the new
//!    one takes its place; anything that goes wrong puts the old name back;
//! 4. the app exits with `EXIT_RESTART`, so the supervisor starts the new
//!    program straight away. The leftover is deleted on the next start.

use super::app;
use super::paths;
use super::ui;
use crate::release::{self, Update, UpdateState, Version};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Where this copy looks for its own releases. A different build (or a fork)
/// points `settings.update_feed` somewhere else.
pub const RELEASES_API: &str = "https://git.zacx.net/api/v1/repos/owner/flex-taskbar/releases?limit=10";

/// How often the automatic check runs.
const CHECK_EVERY: u64 = 24 * 60 * 60;
/// Nothing the API answers with should be anywhere near this large.
const MAX_JSON: usize = 1 << 20;
/// Nor should the program be (it is a few megabytes).
const MAX_EXE: usize = 128 << 20;
/// The name the running program is renamed to while it is replaced.
const OLD_EXE: &str = "FlexTaskbar.old.exe";

/// Who wanted the check, which decides what it says when there is nothing to
/// report. A release that is found is always offered, however it was asked
/// for — bar one the user has skipped, which only an explicit ask re-offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// The daily look, by itself. Says nothing unless there is something new.
    Quietly,
    /// *Check for updates…* in the menu: reports whatever it finds, including
    /// "you already have the newest", because nothing else would.
    FromMenu,
    /// The button in the Manage window, which sits beside a line that reports
    /// the outcome itself — so no dialog for a result that line already gives.
    FromWindow,
}

impl Asked {
    /// Whether the user asked for this one, as opposed to the daily look.
    fn by_hand(self) -> bool {
        self != Asked::Quietly
    }

    /// Whether to put up a dialog for an outcome that isn't an update.
    fn reports_nothing_new(self) -> bool {
        self == Asked::FromMenu
    }
}

/// What a helper thread has finished doing, handled on the UI thread.
pub enum Event {
    /// A check finished, and who asked for it.
    Checked { asked: Asked, result: Result<Option<Update>, String> },
    /// A download finished: the file it was written to, verified.
    Downloaded { update: Box<Update>, result: Result<PathBuf, String> },
}

// ---------------------------------------------------------------- checking

/// Deletes the program the last update replaced. Called at startup, by which
/// time nothing is running from it any more.
pub fn clean_leftovers() {
    if let Some(dir) = paths::get().exe.parent() {
        let _ = std::fs::remove_file(dir.join(OLD_EXE));
    }
}

thread_local! {
    /// How the last look for a newer release went, and what it found. The
    /// Manage window shows this, and can offer the release again without
    /// going back to the forge.
    static STATE: RefCell<(UpdateState, Option<Update>)> = const { RefCell::new((UpdateState::Unknown, None)) };
}

/// How the search for a newer release is going, for the Manage window.
pub fn state() -> UpdateState {
    STATE.with(|s| s.borrow().0)
}

fn set_state(now: UpdateState, found: Option<Update>) {
    STATE.with(|s| *s.borrow_mut() = (now, found));
    // The Manage window shows it, when it is open.
    super::manager::update_state_changed();
}

/// Offers the release the last check found, without asking the forge again.
/// Returns whether there was one to offer.
pub fn offer_found() -> bool {
    let found = STATE.with(|s| s.borrow().1.clone());
    match found {
        Some(update) => {
            offer(update);
            true
        }
        None => false,
    }
}

/// Looks for a newer release once a day, if the setting allows it. The check
/// is quiet: it only says anything when there is something to say.
pub fn check_in_background() {
    let due = app::with(|s| {
        if !s.cfg.settings.check_for_updates {
            return false;
        }
        let now = crate::config::unix_time();
        // A clock that has gone backwards (or a config from the future)
        // shouldn't stop checking for ever.
        if now.saturating_sub(s.cfg.last_update_check) < CHECK_EVERY && s.cfg.last_update_check <= now {
            return false;
        }
        s.cfg.last_update_check = now;
        true
    });
    if due {
        app::save();
        check(Asked::Quietly);
    }
}

/// Asks the forge for its releases. What it says when it finds nothing new
/// depends on who asked (see [`Asked`]).
pub fn check(asked: Asked) {
    set_state(UpdateState::Checking, None);
    let feed = app::with(|s| {
        let feed = s.cfg.settings.update_feed.trim();
        if feed.is_empty() { RELEASES_API.to_string() } else { feed.to_string() }
    });
    thread(move || {
        let result = http::get(&feed, MAX_JSON).and_then(|body| {
            let text = String::from_utf8(body).map_err(|_| "the answer wasn't text".to_string())?;
            Ok(release::newer_than(&text, Version::running()))
        });
        Event::Checked { asked, result }
    });
}

/// A check or a download finished (UI thread).
pub fn event(event: Event) {
    match event {
        Event::Checked { asked, result } => checked(asked, result),
        Event::Downloaded { update, result } => downloaded(*update, result),
    }
}

fn checked(asked: Asked, result: Result<Option<Update>, String>) {
    let running = Version::running();
    // Remember how it went, whoever asked: the Manage window shows it.
    match &result {
        Err(_) => set_state(UpdateState::Failed, None),
        Ok(None) => set_state(UpdateState::UpToDate, None),
        Ok(Some(update)) => set_state(UpdateState::Available(update.version), Some(update.clone())),
    }
    match result {
        Err(e) if asked.by_hand() => ui::warn(
            None,
            &format!(
                "FlexTaskbar couldn't check for updates.

{e}"
            ),
        ),
        // A background check that failed (no network, the forge is down) says
        // nothing: it tries again tomorrow.
        Err(_) => {}
        // The Manage window's own line already says this, so only the menu
        // puts up a dialog for it.
        Ok(None) if asked.reports_nothing_new() => {
            ui::info(None, &format!("FlexTaskbar {running} is the newest version."))
        }
        Ok(None) => {}
        // Asked for: always offer, even a release that was skipped before.
        Ok(Some(update)) if asked.by_hand() => offer(update),
        // Found by itself: offer it, unless it is the one they skipped.
        Ok(Some(update)) if !skipped(&update) => offer(update),
        Ok(Some(_)) => {}
    }
}

/// Whether this is the release the user chose to skip.
fn skipped(update: &Update) -> bool {
    app::with(|s| Version::parse(&s.cfg.skipped_update) == Some(update.version))
}

/// Remembers that this release was skipped, so it isn't offered again by
/// itself. Anything newer still is.
fn skip(update: &Update) {
    app::with(|s| s.cfg.skipped_update = update.version.to_string());
    app::save();
}

const UPDATE_NOW: i32 = 101;
const NOT_NOW: i32 = 102;
const SKIP: i32 = 103;
const RELEASE_PAGE: i32 = 104;

/// Tells the user what is available and lets them install it, leave it for
/// now, or skip this release for good.
fn offer(update: Update) {
    let running = Version::running();
    let heading = format!("FlexTaskbar {} is available", update.version);
    let notes = first_lines(&update.notes, 12);
    if update.sums.is_none() {
        // Nothing to check the download against: never install it, just
        // offer to show them the release.
        let detail = format!(
            "You have {running}. This release was published without a {} file, so this copy can't check what it              downloads and won't install it by itself.

{notes}",
            release::SUMS_ASSET
        );
        let choices = [
            ui::Choice { id: RELEASE_PAGE, label: "Open the release page", note: "Download and replace it yourself" },
            ui::Choice { id: NOT_NOW, label: "Not now", note: "" },
            ui::Choice { id: SKIP, label: "Skip this version", note: "Don't mention this release again" },
        ];
        match ui::choose(None, "FlexTaskbar", &heading, &detail, &choices) {
            Some(RELEASE_PAGE) => app::open_link(&update.page),
            Some(SKIP) => skip(&update),
            _ => {}
        }
        return;
    }
    let size = (update.exe.size as f64 / (1024.0 * 1024.0)).max(0.1);
    let detail = format!(
        "You have {running}.

{notes}"
    );
    let download_note = format!("Downloads {size:.1} MB, checks it, and restarts FlexTaskbar");
    let choices = [
        ui::Choice { id: UPDATE_NOW, label: "Update now", note: &download_note },
        ui::Choice { id: NOT_NOW, label: "Not now", note: "Asked again tomorrow" },
        ui::Choice { id: SKIP, label: "Skip this version", note: "Don't mention this release again" },
    ];
    match ui::choose(None, "FlexTaskbar", &heading, &detail, &choices) {
        Some(UPDATE_NOW) => {}
        Some(SKIP) => return skip(&update),
        // Closed, left for now, or the dialog couldn't be shown.
        _ => return,
    }
    app::balloon(&format!("Downloading FlexTaskbar {}…", update.version), false);
    thread(move || {
        let result = download(&update);
        Event::Downloaded { update: Box::new(update), result }
    });
}

/// Downloads the program and checks it against the published checksum.
fn download(update: &Update) -> Result<PathBuf, String> {
    let sums = update.sums.as_ref().ok_or("the release has no checksums")?;
    let sums = http::get(&sums.url, MAX_JSON)?;
    let sums = String::from_utf8(sums).map_err(|_| "the checksums file wasn't text".to_string())?;
    let want = release::sha256_of(&sums, &update.exe.name)
        .ok_or_else(|| format!("{} isn't listed in {}", update.exe.name, release::SUMS_ASSET))?;

    let body = http::get(&update.exe.url, MAX_EXE)?;
    let got = sha256(&body);
    if got != want {
        return Err(format!(
            "the download doesn't match the checksum published with it, so it was thrown away.\n\n\
             Expected {want}\nGot      {got}"
        ));
    }

    let dir = paths::get().data.join("update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} couldn't be made: {e}", dir.display()))?;
    let file = dir.join(format!("FlexTaskbar-{}.exe", update.version));
    std::fs::write(&file, &body).map_err(|e| format!("{} couldn't be written: {e}", file.display()))?;
    Ok(file)
}

fn downloaded(update: Update, result: Result<PathBuf, String>) {
    let file = match result {
        Ok(file) => file,
        Err(e) => {
            ui::error(None, &format!("FlexTaskbar {} couldn't be downloaded.\n\n{e}", update.version));
            return;
        }
    };
    match install(&file) {
        Ok(()) => {
            // Everything is in place: the supervisor starts the new program.
            app::restart_for_update(update.version);
        }
        Err(e) => {
            ui::error(
                None,
                &format!(
                    "FlexTaskbar {} was downloaded and checked, but couldn't be put in place.\n\n{e}\n\n\
                     The new program is in {} — you can replace {} with it yourself.",
                    update.version,
                    file.display(),
                    paths::get().exe.display()
                ),
            );
        }
    }
}

/// Puts `new` where the running program is: the running image is renamed
/// aside first, and put back if anything fails.
fn install(new: &Path) -> Result<(), String> {
    let exe = &paths::get().exe;
    let dir = exe.parent().ok_or("the program isn't in a folder")?;
    let aside = dir.join(OLD_EXE);
    // A leftover from an earlier update would block the rename.
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(exe, &aside).map_err(|e| format!("{} couldn't be moved aside: {e}", exe.display()))?;
    match std::fs::copy(new, exe) {
        Ok(_) => {
            let _ = std::fs::remove_file(new);
            Ok(())
        }
        Err(e) => {
            // Put the running program back, or there would be none at all.
            let _ = std::fs::rename(&aside, exe);
            Err(format!("{} couldn't be written: {e}", exe.display()))
        }
    }
}

/// The first `n` lines of the release notes, for a message box.
fn first_lines(notes: &str, n: usize) -> String {
    let mut lines: Vec<&str> = notes.lines().take(n + 1).collect();
    let more = lines.len() > n;
    lines.truncate(n);
    let text = lines.join("\n");
    let text = text.trim();
    match (text.is_empty(), more) {
        (true, _) => "No release notes.".to_string(),
        (false, true) => format!("{text}\n…"),
        (false, false) => text.to_string(),
    }
}

/// Runs `work` on a helper thread and posts what it returns to the main
/// window. The boxed event is freed here if the window has gone.
fn thread(work: impl FnOnce() -> Event + Send + 'static) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    let main_raw = app::main_hwnd().0 as isize;
    std::thread::spawn(move || {
        let ptr = Box::into_raw(Box::new(work()));
        unsafe {
            let hwnd = HWND(main_raw as *mut _);
            if PostMessageW(Some(hwnd), app::WM_APP_UPDATE, WPARAM(0), LPARAM(ptr as isize)).is_err() {
                drop(Box::from_raw(ptr));
            }
        }
    });
}

// ---------------------------------------------------------------- hashing

/// The SHA-256 of `data` as lower-case hex, using Windows' own CNG.
fn sha256(data: &[u8]) -> String {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_ALG_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
        BCryptHash, BCryptOpenAlgorithmProvider,
    };
    use windows::core::PCWSTR;
    let mut alg = BCRYPT_ALG_HANDLE::default();
    let mut out = [0u8; 32];
    unsafe {
        let opened = BCryptOpenAlgorithmProvider(
            &mut alg,
            BCRYPT_SHA256_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        );
        if opened.0 != 0 {
            return String::new();
        }
        let hashed = BCryptHash(alg, None, data, &mut out).0 == 0;
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        if !hashed {
            return String::new();
        }
    }
    out.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- fetching

/// The little HTTPS client this needs: one GET, into memory, with a cap.
mod http {
    use crate::release::Version;
    use windows::Win32::Networking::WinHttp::{
        URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_INTERNET_SCHEME_HTTPS,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetTimeouts,
    };
    use windows::core::PCWSTR;

    /// Closes a WinHTTP handle however the function returns.
    struct Handle(*mut core::ffi::c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    /// Fetches `url`, giving up after `limit` bytes. Only HTTPS: an update
    /// must not arrive over a connection anyone can rewrite.
    pub fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
        let (host, port, path) = split(url)?;
        let agent = super::ui::wide(&format!("FlexTaskbar/{}", Version::running()));
        let host_w = super::ui::wide(&host);
        let path_w = super::ui::wide(&path);
        unsafe {
            let session = Handle(WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ));
            if session.0.is_null() {
                return Err("no connection could be opened".into());
            }
            // Nothing here is worth waiting on for long: the check is quiet
            // and the download can be tried again.
            let _ = WinHttpSetTimeouts(session.0, 10_000, 15_000, 30_000, 60_000);
            let connect = Handle(WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0));
            if connect.0.is_null() {
                return Err(format!("{host} couldn't be reached"));
            }
            let verb = super::ui::wide("GET");
            let request = Handle(WinHttpOpenRequest(
                connect.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if request.0.is_null() {
                return Err("the request couldn't be made".into());
            }
            let accept = super::ui::wide("Accept: application/json, application/octet-stream\r\n");
            WinHttpSendRequest(request.0, Some(&accept[..accept.len() - 1]), None, 0, 0, 0)
                .map_err(|e| format!("{host} didn't answer ({})", e.message()))?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut())
                .map_err(|e| format!("{host} didn't answer ({})", e.message()))?;

            let mut status: u32 = 0;
            let mut len = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut _),
                &mut len,
                std::ptr::null_mut(),
            )
            .map_err(|_| "the answer had no status".to_string())?;
            if status == 404 {
                return Err("there are no releases to see (the forge answered 404)".into());
            }
            if !(200..300).contains(&status) {
                return Err(format!("the forge answered {status}"));
            }

            let mut body = Vec::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let mut read: u32 = 0;
                WinHttpReadData(request.0, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut read)
                    .map_err(|e| format!("the download stopped ({})", e.message()))?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read as usize]);
                if body.len() > limit {
                    return Err("the download was larger than expected".into());
                }
            }
            Ok(body)
        }
    }

    /// Host, port and path of an HTTPS URL.
    fn split(url: &str) -> Result<(String, u16, String), String> {
        let wide = super::ui::wide(url);
        let mut host = [0u16; 256];
        let mut path = [0u16; 2048];
        let mut extra = [0u16; 2048];
        let mut parts = URL_COMPONENTS {
            dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
            lpszHostName: windows::core::PWSTR(host.as_mut_ptr()),
            dwHostNameLength: host.len() as u32,
            lpszUrlPath: windows::core::PWSTR(path.as_mut_ptr()),
            dwUrlPathLength: path.len() as u32,
            lpszExtraInfo: windows::core::PWSTR(extra.as_mut_ptr()),
            dwExtraInfoLength: extra.len() as u32,
            ..Default::default()
        };
        unsafe {
            WinHttpCrackUrl(&wide[..wide.len() - 1], 0, &mut parts).map_err(|_| format!("{url} isn't a usable URL"))?;
        }
        if parts.nScheme != WINHTTP_INTERNET_SCHEME_HTTPS {
            return Err("updates are only fetched over HTTPS".into());
        }
        let text = |p: &windows::core::PWSTR, n: u32| {
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p.0, n as usize) })
        };
        let host = text(&parts.lpszHostName, parts.dwHostNameLength);
        let mut full = text(&parts.lpszUrlPath, parts.dwUrlPathLength);
        full.push_str(&text(&parts.lpszExtraInfo, parts.dwExtraInfoLength));
        if host.is_empty() {
            return Err(format!("{url} has no host"));
        }
        Ok((host, parts.nPort, if full.is_empty() { "/".to_string() } else { full }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_notes_say_so_and_long_ones_are_cut() {
        assert_eq!(first_lines("", 3), "No release notes.");
        assert_eq!(first_lines("one\ntwo", 3), "one\ntwo");
        assert_eq!(first_lines("1\n2\n3\n4\n5", 3), "1\n2\n3\n…");
    }

    /// Not part of the normal run (it needs the network and the forge):
    /// `cargo test -- --ignored fetches` checks the WinHTTP path by hand.
    /// The live feed, read by the same code the app uses: the published
    /// release must parse, and offer itself to an older copy.
    #[test]
    #[ignore = "needs the network"]
    fn reads_the_published_releases() {
        let body = http::get(RELEASES_API, MAX_JSON).expect("the releases");
        let json = String::from_utf8(body).expect("text");
        let older = Version { major: 2, minor: 0, patch: 0 };
        let found = release::newer_than(&json, older).expect("an update for 2.0.0");
        assert!(found.version > older, "{:?}", found.version);
        assert_eq!(found.exe.name, release::EXE_ASSET);
        assert!(found.exe.url.starts_with("https://"), "{}", found.exe.url);
        assert!(found.sums.is_some(), "a release must publish checksums");
        // The running copy is current, so nothing is offered to it.
        assert_eq!(release::newer_than(&json, found.version), None);
    }

    /// Everything an install does bar replacing the program: the published
    /// release is downloaded and checked against its published checksum.
    #[test]
    #[ignore = "needs the network"]
    fn downloads_and_verifies_a_published_release() {
        let body = http::get(RELEASES_API, MAX_JSON).expect("the releases");
        let json = String::from_utf8(body).expect("text");
        let older = Version { major: 0, minor: 0, patch: 0 };
        let found = release::newer_than(&json, older).expect("an update");
        let file = download(&found).expect("a verified download");
        let size = std::fs::metadata(&file).expect("the file").len();
        assert_eq!(size, found.exe.size, "{} is the wrong size", file.display());
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    #[ignore = "needs the network"]
    fn fetches_over_https() {
        let body = http::get("https://git.zacx.net/api/v1/version", 4096).expect("a version");
        let text = String::from_utf8(body).expect("text");
        assert!(text.contains("version"), "{text}");
        assert!(http::get("http://git.zacx.net/api/v1/version", 4096).is_err(), "plain HTTP must be refused");
    }

    #[test]
    fn sha256_matches_the_known_digest_of_the_empty_input() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
