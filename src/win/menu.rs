//! The launcher menu: native popup menus, one submenu per category at any
//! depth. Native menus open instantly, scroll when long, and come with full
//! keyboard navigation.

use super::app::{self, FOLDER_ICON};
use super::autostart;
use super::ui::{escape_amp, wide};
use crate::config::Category;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, HMENU, InsertMenuItemW, MENU_ITEM_STATE, MENUITEMINFOW,
    MFS_CHECKED, MFS_DISABLED, MFT_SEPARATOR, MFT_STRING, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING,
    MIIM_SUBMENU, PostMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_NULL,
};
use windows::core::PWSTR;

pub enum Action {
    Launch(String),
    Search,
    Manage,
    Rescan,
    ToggleAutostart,
    OpenDataFolder,
    Exit,
}

struct Builder {
    actions: Vec<Action>,
    /// Keeps item text alive until the menu is built.
    strings: Vec<Vec<u16>>,
}

impl Builder {
    fn popup() -> HMENU {
        // No MNS_CHECKORBMP: giving icons their own column (the standard Vista+
        // layout) keeps them from overlapping the text when the check column is
        // narrower than the icon.
        unsafe { CreatePopupMenu().unwrap_or_default() }
    }

    fn command(&mut self, action: Action) -> u32 {
        self.actions.push(action);
        self.actions.len() as u32 // ids start at 1; 0 means "cancelled"
    }

    fn item(
        &mut self,
        menu: HMENU,
        text: &str,
        id: u32,
        sub: Option<HMENU>,
        bmp: Option<HBITMAP>,
        state: MENU_ITEM_STATE,
    ) {
        let mut w = wide(text);
        let mut mii = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_STRING | MIIM_ID | MIIM_STATE,
            fType: MFT_STRING,
            fState: state,
            wID: id,
            dwTypeData: PWSTR(w.as_mut_ptr()),
            ..Default::default()
        };
        if let Some(sub) = sub {
            mii.fMask |= MIIM_SUBMENU;
            mii.hSubMenu = sub;
        }
        if let Some(bmp) = bmp {
            mii.fMask |= MIIM_BITMAP;
            mii.hbmpItem = bmp;
        }
        unsafe {
            let pos = GetMenuItemCount(Some(menu)).max(0) as u32;
            let _ = InsertMenuItemW(menu, pos, true, &mii);
        }
        self.strings.push(w);
    }

    fn separator(&mut self, menu: HMENU) {
        let mii = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE,
            fType: MFT_SEPARATOR,
            ..Default::default()
        };
        unsafe {
            let pos = GetMenuItemCount(Some(menu)).max(0) as u32;
            let _ = InsertMenuItemW(menu, pos, true, &mii);
        }
    }

    fn app(&mut self, menu: HMENU, s: &app::State, id: &str) {
        match s.catalog.get(id) {
            Some(entry) => {
                let cmd = self.command(Action::Launch(id.to_string()));
                let bmp = icon(s, id);
                self.item(menu, &escape_amp(&entry.name), cmd, None, bmp, MENU_ITEM_STATE(0));
            }
            None => {
                // Uninstalled since it was filed: show it greyed out instead of hiding it.
                self.item(menu, &format!("{} (not found)", short_id(id)), 0, None, None, MFS_DISABLED);
            }
        }
    }

    fn category(&mut self, s: &app::State, c: &Category) -> HMENU {
        let menu = Self::popup();
        for child in &c.children {
            let sub = self.category(s, child);
            let bmp = category_icon(s, child);
            self.item(menu, &escape_amp(&child.name), 0, Some(sub), bmp, MENU_ITEM_STATE(0));
        }
        if !c.children.is_empty() && !c.apps.is_empty() {
            self.separator(menu);
        }
        for id in &c.apps {
            self.app(menu, s, id);
        }
        if c.children.is_empty() && c.apps.is_empty() {
            self.item(menu, "(empty)", 0, None, None, MFS_DISABLED);
        }
        menu
    }

    fn all_apps(&mut self, s: &app::State) -> HMENU {
        let menu = Self::popup();
        let apps = &s.catalog.apps;
        if apps.is_empty() {
            let text = if s.scanning { "Scanning…" } else { "(no apps found)" };
            self.item(menu, text, 0, None, None, MFS_DISABLED);
            return menu;
        }
        if apps.len() <= s.cfg.settings.group_all_apps_above {
            for a in apps {
                self.app(menu, s, &a.id);
            }
            return menu;
        }
        // A–Z submenus. The catalog is already sorted by name.
        let mut current: Option<(char, HMENU)> = None;
        for a in apps {
            let letter = a.name.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c)).unwrap_or('#');
            let letter = if letter.is_alphabetic() { letter } else { '#' };
            if current.map(|(l, _)| l) != Some(letter) {
                if let Some((l, m)) = current.take() {
                    self.item(menu, &l.to_string(), 0, Some(m), None, MENU_ITEM_STATE(0));
                }
                current = Some((letter, Self::popup()));
            }
            let (_, sub) = current.unwrap();
            self.app(sub, s, &a.id);
        }
        if let Some((l, m)) = current {
            self.item(menu, &l.to_string(), 0, Some(m), None, MENU_ITEM_STATE(0));
        }
        menu
    }
}

fn icon(s: &app::State, app_id: &str) -> Option<HBITMAP> {
    s.icon_of(app_id)
}

fn category_icon(s: &app::State, c: &Category) -> Option<HBITMAP> {
    let key = format!("cat:{}", c.id);
    s.icon_of(&key).or_else(|| s.icon_of(FOLDER_ICON))
}

fn short_id(id: &str) -> String {
    id.rsplit(['\\', '!']).next().unwrap_or(id).to_string()
}

/// Shows the menu at `pt` and returns the chosen action.
pub fn track(owner: HWND, pt: POINT) -> Option<Action> {
    let autostart_on = autostart::is_enabled();
    let (root, mut actions) = app::with(|s| {
        let mut b = Builder { actions: Vec::new(), strings: Vec::new() };
        let root = Builder::popup();

        if s.cfg.settings.show_recents_in_menu {
            let recents: Vec<String> = s.cfg.recents.iter().filter(|id| s.catalog.get(id).is_some()).cloned().collect();
            if !recents.is_empty() {
                let sub = Builder::popup();
                for id in &recents {
                    b.app(sub, s, id);
                }
                b.item(root, "Recent", 0, Some(sub), None, MENU_ITEM_STATE(0));
                b.separator(root);
            }
        }

        if s.cfg.categories.is_empty() {
            b.item(root, "No categories yet — use Manage categories…", 0, None, None, MFS_DISABLED);
        }
        for c in &s.cfg.categories {
            let sub = b.category(s, c);
            let bmp = category_icon(s, c);
            b.item(root, &escape_amp(&c.name), 0, Some(sub), bmp, MENU_ITEM_STATE(0));
        }
        let all = b.all_apps(s);
        b.item(root, "All apps", 0, Some(all), None, MENU_ITEM_STATE(0));

        b.separator(root);
        let search_label = match s.cfg.settings.search_hotkey {
            Some(hk) => format!("Search…\t{}", hk.describe()),
            None => "Search…".to_string(),
        };
        let id = b.command(Action::Search);
        b.item(root, &search_label, id, None, None, MENU_ITEM_STATE(0));
        let id = b.command(Action::Manage);
        b.item(root, "Manage categories…", id, None, None, MENU_ITEM_STATE(0));
        let id = b.command(Action::Rescan);
        let (label, state) =
            if s.scanning { ("Scanning apps…", MFS_DISABLED) } else { ("Rescan apps", MENU_ITEM_STATE(0)) };
        b.item(root, label, id, None, None, state);
        let id = b.command(Action::ToggleAutostart);
        let state = if autostart_on { MFS_CHECKED } else { MENU_ITEM_STATE(0) };
        b.item(root, "Start with Windows", id, None, None, state);
        let id = b.command(Action::OpenDataFolder);
        b.item(root, "Open data folder", id, None, None, MENU_ITEM_STATE(0));
        b.separator(root);
        let id = b.command(Action::Exit);
        b.item(root, "Exit", id, None, None, MENU_ITEM_STATE(0));
        (root, b.actions)
    });

    // State is released here: the menu's modal loop dispatches other messages.
    let chosen = unsafe {
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(owner);
        let id = TrackPopupMenuEx(root, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(root); // destroys every submenu too; bitmaps stay owned by the cache
        id
    };
    if chosen == 0 || chosen > actions.len() {
        return None;
    }
    Some(actions.swap_remove(chosen - 1))
}
