//! The extra entries on an app's right-click menu: the files it was last used
//! on, and the quick actions Windows offers for it.
//!
//! **Recent files** come from `IApplicationDocumentLists`, the documented way
//! to read an app's jump list. It is keyed by the app's AppUserModelID, which
//! for an Apps-folder app is the parsing name the catalog already stores.
//! Items come back as shell items rather than links, so opening one opens the
//! document itself — which lands in the app in question anyway, since it is
//! that app's list the file came from. An app found only as a file on disk
//! (the Start Menu fallback, and custom apps) has no AppUserModelID to ask
//! by, so it lists no files.
//!
//! **Quick actions** are not guessed. The shell is asked for the app's own
//! context-menu verbs and only the ones it reports are offered, so a Store app
//! never offers *Open file location*, and *Settings* — which cannot be
//! removed — never offers *Uninstall*. A verb is invoked by its place in the
//! shell's own menu, so no verb name is ever passed as text.
//!
//! Reading the list and the verbs happens while the menu opens, on the UI
//! thread, which COM is already initialised on (see `app::run`). The shell
//! reads a small local file for the jump list, so it is quick; the number of
//! files asked for is capped all the same. Carrying an action out goes on a
//! background thread: an uninstaller or a UAC prompt must not hold up the bar.

use super::ui::{from_wide, wide};
use crate::jumplist::{Action, Entry, entries, menu_name};
use std::path::{Path, PathBuf};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree,
};
use windows::Win32::UI::Shell::Common::IObjectArray;
use windows::Win32::UI::Shell::{
    ADLT_RECENT, ApplicationDocumentLists, BHID_SFUIObject, CMF_NORMAL, CMINVOKECOMMANDINFO, GCS_VERBW,
    IApplicationDocumentLists, IContextMenu, IShellItem, SHCreateItemFromParsingName, SHOpenFolderAndSelectItems,
    SHParseDisplayName, SIGDN, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID, HMENU, SW_SHOWNORMAL,
};
use windows::core::{PCSTR, PCWSTR, PSTR};

/// At most this many recent files, however many the app reports.
const MAX_RECENT: usize = 10;
/// Long file names are shortened to this many characters in the menu.
const NAME_CHARS: usize = 44;

/// One file from an app's jump list.
pub struct Recent {
    /// How it reads in the menu: shortened, with `&` already doubled.
    label: String,
    path: PathBuf,
}

/// How a quick action is carried out.
enum How {
    /// The place of the verb in the shell's own context menu for the app.
    Verb(u32),
    /// Show the app's own file in Explorer, selected. Used when the shell
    /// offers no *Open file location* verb but we know where the file is.
    Select(PathBuf),
}

/// What identifies the app to the shell.
enum Which {
    /// An item of `shell:AppsFolder` — its parsing name, which doubles as the
    /// app id its jump list is kept under.
    AppsFolder(String),
    /// A program or shortcut on disk.
    File(PathBuf),
}

impl Which {
    /// The path to hand the shell to get this app's item.
    fn parsing_name(&self) -> String {
        match self {
            // Windows' own separator, so there is no escape to get wrong.
            Which::AppsFolder(name) => format!("shell:AppsFolder{}{name}", std::path::MAIN_SEPARATOR),
            Which::File(path) => path.display().to_string(),
        }
    }

    /// The app id its jump list is kept under, if it has one.
    fn app_id(&self) -> Option<&str> {
        match self {
            Which::AppsFolder(name) => Some(name),
            // Windows keys a plain program's list by a hash of its shortcut,
            // which isn't ours to work out.
            Which::File(_) => None,
        }
    }

    fn file(&self) -> Option<&Path> {
        match self {
            Which::File(path) => Some(path),
            Which::AppsFolder(_) => None,
        }
    }
}

/// The extra part of an app's menu: the rows to add, and what each id does.
pub struct Extras {
    first: usize,
    which: Option<Which>,
    recent: Vec<Recent>,
    actions: Vec<(Action, How)>,
    /// The rows to insert, in order. An id of 0 means a separator.
    pub rows: Vec<(usize, String)>,
}

impl Extras {
    /// Nothing to add — for a menu that isn't an app's.
    pub fn none() -> Extras {
        Extras { first: 0, which: None, recent: Vec::new(), actions: Vec::new(), rows: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Carries out the entry `id` if it is one of these. Returns whether it was.
    pub fn chosen(&self, id: usize) -> bool {
        if id < self.first || id >= self.first + self.recent.len() + self.actions.len() {
            return false;
        }
        let i = id - self.first;
        match self.recent.get(i) {
            Some(r) => open_file(&r.path),
            None => match self.actions.get(i - self.recent.len()) {
                Some((_, How::Verb(offset))) => invoke_verb(self.which.as_ref(), *offset),
                Some((_, How::Select(path))) => show_in_explorer(path),
                None => return false,
            },
        }
        true
    }
}

/// Works out what an app's menu should offer. `first` is the first menu id to
/// use; this part of the menu may use up to `IDS` ids from there.
pub fn extras(app_id: &str, first: usize) -> Extras {
    let which = which(app_id);
    let recent = which.as_ref().and_then(Which::app_id).map(|id| recent_files(id, MAX_RECENT)).unwrap_or_default();
    let actions = which.as_ref().map(offered).unwrap_or_default();

    let kinds: Vec<Action> = actions.iter().map(|(a, _)| *a).collect();
    let mut rows = Vec::new();
    let mut next = first;
    for entry in entries(recent.len(), &kinds, MAX_RECENT) {
        match entry {
            Entry::Recent(i) => {
                rows.push((next, recent[i].label.clone()));
                next += 1;
            }
            Entry::Separator => rows.push((0, String::new())),
            Entry::Action(a) => {
                rows.push((next, a.label().to_string()));
                next += 1;
            }
        }
    }
    Extras { first, which, recent, actions, rows }
}

/// Which shell item stands for this app.
fn which(app_id: &str) -> Option<Which> {
    use super::catalog::Source;
    super::app::with(|s| {
        let entry = s.catalog.get(app_id)?;
        match &entry.source {
            Source::Shell(name) => Some(Which::AppsFolder(name.clone())),
            Source::File(path) => Some(Which::File(PathBuf::from(path))),
            Source::Custom => {
                // A custom app may point at a URL or a `shell:` path; only a
                // real file on disk has a location to open, or a program to
                // run as administrator.
                let c = s.cfg.custom_app(app_id)?;
                let path = PathBuf::from(super::launch::expand(c.target.trim()));
                path.is_file().then_some(Which::File(path))
            }
        }
    })
}

// ---------------------------------------------------------------- jump lists

/// The files an app was last used on, newest first.
fn recent_files(app_id: &str, want: usize) -> Vec<Recent> {
    let id = wide(app_id);
    let mut out = Vec::new();
    unsafe {
        let Ok(lists) =
            CoCreateInstance::<_, IApplicationDocumentLists>(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER)
        else {
            return out;
        };
        if lists.SetAppID(PCWSTR(id.as_ptr())).is_err() {
            return out;
        }
        let Ok(arr) = lists.GetList::<IObjectArray>(ADLT_RECENT, want as u32) else { return out };
        for i in 0..arr.GetCount().unwrap_or(0) {
            let Ok(item) = arr.GetAt::<IShellItem>(i) else { continue };
            // Only files that can be opened by path: anything else (a cloud
            // placeholder, a file since deleted) would do nothing when clicked.
            let Some(path) = display_name(&item, SIGDN_FILESYSPATH).map(PathBuf::from) else { continue };
            let name = display_name(&item, SIGDN_NORMALDISPLAY)
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            out.push(Recent { label: super::ui::escape_amp(&menu_name(&name, NAME_CHARS)), path });
        }
    }
    out
}

fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        if p.is_null() {
            return None;
        }
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.filter(|s| !s.is_empty())
    }
}

// ---------------------------------------------------------------- quick actions

/// The app's own context menu, filled in by the shell. The ids in it only
/// mean anything to the object that filled them, so the two are kept together.
fn context_menu(parsing: &str) -> Option<(IContextMenu, HMENU)> {
    let path = wide(parsing);
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()?;
        let cm: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject).ok()?;
        let menu = CreatePopupMenu().ok()?;
        // Ids from 1 up, so an id of 0 ("no command") can't be mistaken for one.
        if cm.QueryContextMenu(menu, 0, 1, 0x7FFF, CMF_NORMAL).is_err() {
            let _ = DestroyMenu(menu);
            return None;
        }
        Some((cm, menu))
    }
}

/// Which quick actions Windows offers for this app, always in menu order.
fn offered(which: &Which) -> Vec<(Action, How)> {
    let mut found: Vec<(Action, How)> = Vec::new();
    if let Some((cm, menu)) = context_menu(&which.parsing_name()) {
        unsafe {
            for pos in 0..GetMenuItemCount(Some(menu)).max(0) {
                let id = GetMenuItemID(menu, pos);
                // Separators and submenus report no usable id.
                if id == 0 || id == u32::MAX {
                    continue;
                }
                let offset = id - 1;
                let mut buf = [0u16; 128];
                let got = cm.GetCommandString(
                    offset as usize,
                    GCS_VERBW,
                    None,
                    PSTR(buf.as_mut_ptr() as *mut u8),
                    buf.len() as u32,
                );
                if got.is_err() {
                    continue;
                }
                if let Some(action) = Action::from_verb(&from_wide(&buf))
                    && !found.iter().any(|(a, _)| *a == action)
                {
                    found.push((action, How::Verb(offset)));
                }
            }
            let _ = DestroyMenu(menu);
        }
    }
    // A program listed only as a file has no *Open file location* verb — it is
    // the file — so show it in Explorer ourselves.
    if !found.iter().any(|(a, _)| *a == Action::OpenFileLocation)
        && let Some(path) = which.file()
    {
        found.push((Action::OpenFileLocation, How::Select(path.to_path_buf())));
    }
    found.sort_by_key(|(a, _)| crate::jumplist::ACTIONS.iter().position(|x| x == a).unwrap_or(usize::MAX));
    found
}

/// Runs the verb at `offset` of the app's own context menu. The menu is asked
/// for again rather than kept: it belongs to the shell, and ours may have been
/// open a while.
fn invoke_verb(which: Option<&Which>, offset: u32) {
    let Some(parsing) = which.map(Which::parsing_name) else { return };
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let Some((cm, menu)) = context_menu(&parsing) else { return };
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            // A verb given as a number is its place in the menu, so no verb
            // name is passed as text.
            lpVerb: PCSTR(offset as usize as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let _ = cm.InvokeCommand(&info);
        let _ = DestroyMenu(menu);
    });
}

/// Opens Explorer on the folder holding `path`, with the file selected.
fn show_in_explorer(path: &Path) {
    let shown = wide(&path.display().to_string());
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let mut pidl = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(shown.as_ptr()), None, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
            // One item and no folder of its own: Explorer opens the item's
            // parent folder and selects it.
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            CoTaskMemFree(Some(pidl as *const _));
        }
    });
}

/// Opens a file from a jump list, as double-clicking it in Explorer would.
fn open_file(path: &Path) {
    super::launch::spawn(super::launch::Target::Path(path.to_path_buf()), |msg| {
        super::ui::warn(None, &msg);
    });
}
