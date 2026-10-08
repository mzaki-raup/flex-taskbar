#![cfg_attr(not(windows), allow(dead_code))]

//! What kind of app an entry is, worked out from how the shell names it.
//! Pure, so it can be unit-tested off Windows.
//!
//! The shell's Apps folder (shell:AppsFolder, Start's "All apps") lists every
//! app by a parsing name:
//! - Microsoft Store / modern (packaged) apps by their AppUserModelID,
//!   `PackageFamilyName!AppId`, e.g. `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`;
//! - web apps installed from Chrome as `Chrome._crx_<id>` (Chromium,
//!   Brave and other Chromium browsers use the same scheme with their own
//!   prefix), and from Edge as `MSEdge._crx_<id>`;
//! - desktop programs by a path, often under a known-folder GUID.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppKind {
    Desktop,
    /// A Microsoft Store / modern Windows 10 & 11 app (packaged, UWP or MSIX).
    Store,
    ChromeWebApp,
    EdgeWebApp,
    /// Another Chromium browser's web app (Brave, Vivaldi, Chromium…).
    WebApp,
    /// From a package manager's folder (winget portable, Scoop, Chocolatey,
    /// npm, pip…).
    Package(crate::pkgsources::Manager),
    /// Added by the user.
    Custom,
}

impl AppKind {
    /// A short label for lists; empty for ordinary desktop programs.
    pub fn label(self) -> &'static str {
        match self {
            AppKind::Desktop => "",
            AppKind::Store => "Store app",
            AppKind::ChromeWebApp => "Chrome web app",
            AppKind::EdgeWebApp => "Edge web app",
            AppKind::WebApp => "Web app",
            AppKind::Package(m) => m.label(),
            AppKind::Custom => "Custom",
        }
    }
}

/// The browser prefix of a Chromium web app's AppUserModelID (`Chrome._crx_…`),
/// if it is one.
fn crx_browser(aumid: &str) -> Option<&str> {
    let (browser, rest) = aumid.split_once("._crx_")?;
    (!browser.is_empty() && !rest.is_empty() && !browser.contains(['\\', '/'])).then_some(browser)
}

fn web_app_of(browser: &str) -> AppKind {
    let b = browser.to_ascii_lowercase();
    if b == "msedge" || b.starts_with("msedge.") || b == "edge" {
        AppKind::EdgeWebApp
    } else if b == "chrome" || b.starts_with("chrome.") {
        AppKind::ChromeWebApp
    } else {
        AppKind::WebApp
    }
}

/// The kind of an app from the Apps folder, by its parsing name.
pub fn classify_shell(parsing_name: &str) -> AppKind {
    if let Some(browser) = crx_browser(parsing_name) {
        return web_app_of(browser);
    }
    // PackageFamilyName!AppId, where the family name ends in _<publisher id>.
    if let Some((family, app)) = parsing_name.split_once('!')
        && !app.is_empty()
        && !family.contains(['\\', '/'])
        && family.rsplit_once('_').is_some_and(|(name, publisher)| !name.is_empty() && !publisher.is_empty())
    {
        return AppKind::Store;
    }
    AppKind::Desktop
}

/// The kind of a Start Menu shortcut (the fallback scan), by its path.
pub fn classify_file(path: &str) -> AppKind {
    if let Some(m) = crate::pkgsources::manager_of(path) {
        return AppKind::Package(m);
    }
    let lower = path.to_ascii_lowercase().replace('/', "\\");
    if lower.contains("\\chrome apps\\") {
        AppKind::ChromeWebApp
    } else if lower.contains("\\edge apps\\") {
        AppKind::EdgeWebApp
    } else {
        AppKind::Desktop
    }
}

/// The kind of a custom app: a web app if it launches a browser's app shim
/// with `--app-id`, otherwise just custom.
pub fn classify_custom(target: &str, args: &str) -> AppKind {
    if !args.contains("--app-id=") {
        return AppKind::Custom;
    }
    let exe = target.rsplit(['\\', '/']).next().unwrap_or(target).to_ascii_lowercase();
    match exe.as_str() {
        "chrome_proxy.exe" | "chrome.exe" => AppKind::ChromeWebApp,
        "msedge_proxy.exe" | "msedge.exe" => AppKind::EdgeWebApp,
        _ => AppKind::WebApp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_apps() {
        assert_eq!(classify_shell("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), AppKind::Store);
        assert_eq!(classify_shell("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), AppKind::Store);
        assert_eq!(
            classify_shell("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"),
            AppKind::Store
        );
        // Edge PWAs installed as packages are packaged apps too.
        assert_eq!(classify_shell("www.youtube.com-54E21B02_pd8mbgmqs65xy!App"), AppKind::Store);
    }

    #[test]
    fn web_apps() {
        assert_eq!(classify_shell("Chrome._crx_agimnkijcaahngcdmfeangaknmldooml"), AppKind::ChromeWebApp);
        assert_eq!(classify_shell("MSEdge._crx_fmgjjmmmlfnkbppncabfkddbjimcfncm"), AppKind::EdgeWebApp);
        assert_eq!(classify_shell("Brave._crx_abcdefghijklmnop"), AppKind::WebApp);
        assert_eq!(AppKind::EdgeWebApp.label(), "Edge web app");
    }

    #[test]
    fn desktop_programs() {
        assert_eq!(
            classify_shell("{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe"),
            AppKind::Desktop
        );
        assert_eq!(classify_shell("Microsoft.Windows.Explorer"), AppKind::Desktop);
        assert_eq!(classify_shell("Chrome"), AppKind::Desktop);
        // A '!' inside a path is not an AUMID.
        assert_eq!(classify_shell("C:\\Games\\Wow!\\wow.exe"), AppKind::Desktop);
        assert_eq!(AppKind::Desktop.label(), "");
    }

    #[test]
    fn start_menu_and_custom() {
        let choco = "C:\\ProgramData\\chocolatey\\bin\\7z.exe";
        assert_eq!(classify_file(choco), AppKind::Package(crate::pkgsources::Manager::Chocolatey));
        assert_eq!(classify_file(choco).label(), "Chocolatey");
        let p = "C:\\Users\\me\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Chrome Apps\\YouTube.lnk";
        assert_eq!(classify_file(p), AppKind::ChromeWebApp);
        assert_eq!(
            classify_file("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\7-Zip\\7-Zip.lnk"),
            AppKind::Desktop
        );
        let chrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome_proxy.exe";
        assert_eq!(classify_custom(chrome, "--profile-directory=Default --app-id=abc"), AppKind::ChromeWebApp);
        assert_eq!(classify_custom("C:\\Edge\\msedge_proxy.exe", "--app-id=abc"), AppKind::EdgeWebApp);
        assert_eq!(classify_custom(chrome, ""), AppKind::Custom);
        assert_eq!(classify_custom("C:\\Tools\\thing.exe", ""), AppKind::Custom);
    }
}
