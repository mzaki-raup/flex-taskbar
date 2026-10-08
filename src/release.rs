#![cfg_attr(not(windows), allow(dead_code))]

//! Which published release, if any, is newer than the copy that is running.
//!
//! The build workflow tags a release `v2.0.1` and attaches the program and a
//! `SHA256SUMS.txt` to it; the app asks the forge's API for the releases and
//! works out here whether one of them is worth offering. Pure, so it can be
//! unit-tested off Windows: `win::update` does the fetching, checking and
//! installing.

use serde::Deserialize;

/// The program as published on a release, and the file of checksums beside it.
pub const EXE_ASSET: &str = "FlexTaskbar.exe";
pub const SUMS_ASSET: &str = "SHA256SUMS.txt";

/// A release version, as a tag says it (`v2.0.1`, or `2.0.1`). Ordered, so
/// the newest release is simply the largest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        // Anything after the three numbers (`-rc1`, `+build`) is not a release
        // this understands.
        let mut parts = s.split('.');
        let mut next = || parts.next().filter(|p| !p.is_empty())?.parse::<u32>().ok();
        let (major, minor, patch) = (next()?, next()?, next()?);
        parts.next().is_none().then_some(Version { major, minor, patch })
    }

    /// The version this copy was built as (`Cargo.toml`).
    pub fn running() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_default()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A release worth offering: newer than what is running, and with the
/// program attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    pub tag: String,
    /// The release's page, for *What's new*.
    pub page: String,
    pub notes: String,
    pub exe: Asset,
    /// The checksums beside it. A release without them is not installed
    /// automatically (see `win::update`).
    pub sums: Option<Asset>,
}

/// What the forge's releases API answers with. Unknown fields are ignored, so
/// this reads both Forgejo/Gitea and GitHub.
#[derive(Deserialize)]
struct ApiRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl ApiRelease {
    fn asset(&self, name: &str) -> Option<Asset> {
        self.assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name) && !a.browser_download_url.is_empty())
            .map(|a| Asset { name: a.name.clone(), url: a.browser_download_url.clone(), size: a.size })
    }
}

/// The newest release in the API's answer that is newer than `current`.
/// Drafts, pre-releases, tags that aren't a plain version, and releases with
/// no program attached are all passed over.
pub fn newer_than(json: &str, current: Version) -> Option<Update> {
    let releases: Vec<ApiRelease> = serde_json::from_str(json).ok()?;
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(&r.tag_name)?;
            if version <= current {
                return None;
            }
            Some(Update {
                version,
                tag: r.tag_name.clone(),
                page: r.html_url.clone(),
                notes: r.body.trim().to_string(),
                exe: r.asset(EXE_ASSET)?,
                sums: r.asset(SUMS_ASSET),
            })
        })
        .max_by_key(|u| u.version)
}

/// The checksum of `name` in a `SHA256SUMS.txt` ("<hex>  <file name>" lines,
/// as `sha256sum` writes them), lowercased.
pub fn sha256_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        // `sha256sum` marks a binary-mode file with a `*`.
        let file = file.trim().trim_start_matches('*');
        let hash = hash.trim();
        let ok = file.eq_ignore_ascii_case(name) && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        ok.then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version { major, minor, patch }
    }

    #[test]
    fn versions_parse_with_or_without_the_tag_prefix() {
        assert_eq!(Version::parse("v2.0.1"), Some(v(2, 0, 1)));
        assert_eq!(Version::parse("2.10.0"), Some(v(2, 10, 0)));
        assert_eq!(Version::parse(" V0.0.0 "), Some(v(0, 0, 0)));
    }

    #[test]
    fn anything_that_is_not_three_numbers_is_not_a_version() {
        for s in ["", "v2", "2.0", "2.0.1.1", "2.0.x", "v2.0.1-rc1", "nightly", "2..1"] {
            assert_eq!(Version::parse(s), None, "{s} should not parse");
        }
    }

    #[test]
    fn versions_order_by_number_not_by_text() {
        assert!(v(2, 10, 0) > v(2, 9, 9));
        assert!(v(3, 0, 0) > v(2, 99, 99));
        assert!(v(2, 0, 1) > v(2, 0, 0));
    }

    #[test]
    fn the_running_version_is_the_one_in_cargo_toml() {
        assert_eq!(Version::running().to_string(), env!("CARGO_PKG_VERSION"));
    }

    fn release(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"html_url":"https://forge/r/{tag}",
               "body":"notes for {tag}","assets":[
                 {{"name":"FlexTaskbar.exe","browser_download_url":"https://forge/{tag}/FlexTaskbar.exe","size":42}},
                 {{"name":"SHA256SUMS.txt","browser_download_url":"https://forge/{tag}/SHA256SUMS.txt","size":90}}
               ]{extra}}}"#
        )
    }

    #[test]
    fn the_newest_release_newer_than_the_running_one_wins() {
        let json = format!("[{},{},{}]", release("v2.0.1", ""), release("v2.2.0", ""), release("v2.1.0", ""));
        let found = newer_than(&json, v(2, 0, 0)).expect("an update");
        assert_eq!(found.version, v(2, 2, 0));
        assert_eq!(found.tag, "v2.2.0");
        assert_eq!(found.exe.url, "https://forge/v2.2.0/FlexTaskbar.exe");
        assert_eq!(found.sums.map(|a| a.name), Some("SHA256SUMS.txt".to_string()));
        assert_eq!(found.notes, "notes for v2.2.0");
    }

    #[test]
    fn nothing_is_offered_when_the_running_copy_is_current_or_newer() {
        let json = format!("[{}]", release("v2.0.0", ""));
        assert_eq!(newer_than(&json, v(2, 0, 0)), None);
        assert_eq!(newer_than(&json, v(2, 1, 0)), None);
    }

    #[test]
    fn drafts_prereleases_and_odd_tags_are_passed_over() {
        let drafted = release("v9.0.0", "").replace(r#""draft":false"#, r#""draft":true"#);
        let pre = release("v8.0.0", "").replace(r#""prerelease":false"#, r#""prerelease":true"#);
        let odd = release("nightly", "");
        let json = format!("[{drafted},{pre},{odd},{}]", release("v2.0.1", ""));
        assert_eq!(newer_than(&json, v(2, 0, 0)).map(|u| u.version), Some(v(2, 0, 1)));
    }

    #[test]
    fn a_release_without_the_program_attached_is_not_an_update() {
        let bare = r#"[{"tag_name":"v3.0.0","html_url":"https://forge/r","assets":[]}]"#;
        assert_eq!(newer_than(bare, v(2, 0, 0)), None);
    }

    #[test]
    fn a_release_without_checksums_is_still_found() {
        let no_sums = release("v2.0.1", "").replace(
            r#",
                 {"name":"SHA256SUMS.txt","browser_download_url":"https://forge/v2.0.1/SHA256SUMS.txt","size":90}"#,
            "",
        );
        let found = newer_than(&format!("[{no_sums}]"), v(2, 0, 0)).expect("an update");
        assert_eq!(found.sums, None);
    }

    #[test]
    fn rubbish_from_the_api_is_not_an_update() {
        for json in ["", "null", "{}", "[", r#"{"message":"Not Found"}"#] {
            assert_eq!(newer_than(json, v(2, 0, 0)), None, "{json}");
        }
    }

    #[test]
    fn checksums_are_read_by_file_name() {
        let sums = "  \n\
            e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  FlexTaskbar.exe\n\
            ABCDEF0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789 *FlexTaskbar-2.0.1.zip\n";
        assert_eq!(
            sha256_of(sums, "FlexTaskbar.exe").as_deref(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        // The `*` binary marker and upper case both read.
        assert_eq!(
            sha256_of(sums, "FlexTaskbar-2.0.1.zip").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(sha256_of(sums, "nothing.exe"), None);
    }

    #[test]
    fn a_checksum_that_is_not_a_sha256_is_refused() {
        assert_eq!(sha256_of("deadbeef  FlexTaskbar.exe", "FlexTaskbar.exe"), None);
        assert_eq!(
            sha256_of(
                "zzz0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  FlexTaskbar.exe",
                "FlexTaskbar.exe"
            ),
            None
        );
    }
}
