//! Screen readers (Narrator, NVDA, JAWS): the bar and the flyouts are drawn
//! by FlexTaskbar itself, so they describe their buttons through Microsoft
//! Active Accessibility. Each window answers `WM_GETOBJECT` with an
//! `IAccessible` whose children are its buttons, tiles and rows: their names,
//! roles (button, button with a flyout, text), states (focused, under the
//! pointer, open), places on screen and what pressing them does. UI
//! Automation clients read the same through Windows' MSAA proxy.
//!
//! Nothing is kept: every call asks the bar or the flyout for its buttons as
//! they are now, so it can never be out of date. A screen reader's "press"
//! is posted back to the window, so it runs like a click, outside the call.
//!
//! Windows serves these calls on an RPC worker thread, not the thread that
//! owns the window, so the answer is fetched with [`WM_APP_ACC_TREE`]: the
//! window's own thread builds the list and hands it back. The bar and the
//! flyouts keep their state in thread-locals, which that worker thread cannot
//! see — asking it directly there would describe every window as empty.

use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, HWND, LPARAM, LRESULT, RECT, S_FALSE, WPARAM};
use windows::Win32::System::Com::{DISPATCH_FLAGS, DISPPARAMS, EXCEPINFO, IDispatch, IDispatch_Impl, ITypeInfo};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    CreateStdAccessibleObject, IAccessible, IAccessible_Impl, LresultFromObject, NAVDIR_FIRSTCHILD, NAVDIR_LASTCHILD,
    NAVDIR_NEXT, NAVDIR_PREVIOUS, NotifyWinEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_FOCUS, GetWindowThreadProcessId, OBJID_CLIENT, OBJID_WINDOW, PostMessageW, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WM_APP,
};
use windows::core::{BSTR, GUID, Interface, PCWSTR, Result, implement};

/// Posted to the window when a screen reader presses child `wparam`
/// (counted from 0).
pub const WM_APP_ACC_PRESS: u32 = WM_APP + 31;

/// Sent to the window to build its [`Tree`] on the thread that owns it. The
/// window answers with a `Box::into_raw(Box<Tree>)`, which the sender owns.
pub const WM_APP_ACC_TREE: u32 = WM_APP + 32;

/// One button, tile, row or label, as a screen reader sees it.
pub struct Element {
    pub name: String,
    /// `ROLE_SYSTEM_*`.
    pub role: u32,
    /// `STATE_SYSTEM_*`.
    pub state: u32,
    /// On screen.
    pub rect: RECT,
    /// What pressing it does ("Open", "Launch"…); empty for text.
    pub action: &'static str,
    /// Extra words, such as "Running" or the app's kind.
    pub description: String,
}

/// The window itself, and its children in order.
pub struct Tree {
    pub name: String,
    pub role: u32,
    pub children: Vec<Element>,
}

/// Which window: each answers for its own buttons.
#[derive(Clone, Copy)]
pub enum Source {
    Bar,
    Flyout,
}

/// The window's buttons as they are now. Only call this on the thread that
/// owns `hwnd`; everywhere else goes through [`tree`].
pub fn build_tree(source: Source, hwnd: HWND) -> Tree {
    match source {
        Source::Bar => super::strip::accessible(),
        Source::Flyout => super::flyout::accessible(hwnd),
    }
}

/// The window's buttons, asked for on the thread that owns it.
fn tree(source: Source, hwnd: HWND) -> Tree {
    let owner = unsafe { GetWindowThreadProcessId(hwnd, None) };
    if owner == unsafe { GetCurrentThreadId() } {
        return build_tree(source, hwnd);
    }
    let mut answer = 0usize;
    // A timeout rather than a plain send: a hung UI thread must not hang the
    // screen reader with it.
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_APP_ACC_TREE,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            ACC_TREE_TIMEOUT_MS,
            Some(&mut answer),
        )
    };
    if sent.0 == 0 || answer == 0 {
        return empty_tree(source);
    }
    *unsafe { Box::from_raw(answer as *mut Tree) }
}

const ACC_TREE_TIMEOUT_MS: u32 = 2_000;

/// What to say when the window's own thread can't answer.
fn empty_tree(source: Source) -> Tree {
    use windows::Win32::UI::Accessibility::{ROLE_SYSTEM_PANE, ROLE_SYSTEM_TOOLBAR};
    match source {
        Source::Bar => Tree { name: "FlexTaskbar".into(), role: ROLE_SYSTEM_TOOLBAR, children: Vec::new() },
        Source::Flyout => Tree { name: "Flyout".into(), role: ROLE_SYSTEM_PANE, children: Vec::new() },
    }
}

/// `WM_APP_ACC_TREE`, on the window's own thread: the tree, for the sender to own.
pub fn tree_message(source: Source, hwnd: HWND) -> LRESULT {
    LRESULT(Box::into_raw(Box::new(build_tree(source, hwnd))) as isize)
}

/// `WM_GETOBJECT`: the window's `IAccessible`, when that is what is asked for.
pub fn get_object(hwnd: HWND, source: Source, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if lparam.0 as i32 != OBJID_CLIENT.0 {
        return None;
    }
    let acc: IAccessible = Accessible { hwnd, source }.into();
    Some(unsafe { LresultFromObject(&IAccessible::IID, wparam, &acc) })
}

/// The keyboard focus moved to child `index` of `hwnd`: screen readers
/// announce it.
pub fn focus_moved(hwnd: HWND, index: usize) {
    unsafe { NotifyWinEvent(EVENT_OBJECT_FOCUS, hwnd, OBJID_CLIENT.0, index as i32 + 1) };
}

#[implement(IAccessible)]
struct Accessible {
    hwnd: HWND,
    source: Source,
}

/// The child a VARIANT names: `Some(None)` for the window itself, `Some(Some(i))`
/// for child `i` (from 0), `None` if there is no such child.
fn which(v: &VARIANT, count: usize) -> Option<Option<usize>> {
    if v.vt() != VT_I4 {
        return None;
    }
    // Read directly: `i32::try_from` goes through propsys, which isn't
    // everywhere (Wine), for what is just the VT_I4 field.
    let n = unsafe { v.Anonymous.Anonymous.Anonymous.lVal };
    match n {
        0 => Some(None),
        n if n > 0 && (n as usize) <= count => Some(Some(n as usize - 1)),
        _ => None,
    }
}

fn child_variant(index: usize) -> VARIANT {
    VARIANT::from(index as i32 + 1)
}

impl Accessible_Impl {
    fn with_child<R>(&self, v: &VARIANT, f: impl FnOnce(&Tree, Option<&Element>) -> R) -> Result<R> {
        let tree = tree(self.source, self.hwnd);
        let Some(which) = which(v, tree.children.len()) else { return Err(E_INVALIDARG.into()) };
        let child = which.map(|i| &tree.children[i]);
        Ok(f(&tree, child))
    }
}

impl IDispatch_Impl for Accessible_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }
    fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Invoke(
        &self,
        _: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        _: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IAccessible_Impl for Accessible_Impl {
    fn accParent(&self) -> Result<IDispatch> {
        // The window object Windows makes for every window.
        let mut out: *mut core::ffi::c_void = std::ptr::null_mut();
        unsafe {
            CreateStdAccessibleObject(self.hwnd, OBJID_WINDOW.0, &IDispatch::IID, &mut out)?;
            Ok(IDispatch::from_raw(out))
        }
    }

    fn accChildCount(&self) -> Result<i32> {
        Ok(tree(self.source, self.hwnd).children.len() as i32)
    }

    fn get_accChild(&self, _: &VARIANT) -> Result<IDispatch> {
        // Every child is a simple element of this object.
        Err(S_FALSE.into())
    }

    fn get_accName(&self, v: &VARIANT) -> Result<BSTR> {
        self.with_child(v, |t, c| BSTR::from(c.map_or(t.name.as_str(), |c| c.name.as_str())))
    }

    fn get_accValue(&self, _: &VARIANT) -> Result<BSTR> {
        Err(S_FALSE.into())
    }

    fn get_accDescription(&self, v: &VARIANT) -> Result<BSTR> {
        match self.with_child(v, |_, c| c.map(|c| c.description.clone()).unwrap_or_default())? {
            d if d.is_empty() => Err(S_FALSE.into()),
            d => Ok(BSTR::from(d)),
        }
    }

    fn get_accRole(&self, v: &VARIANT) -> Result<VARIANT> {
        self.with_child(v, |t, c| VARIANT::from(c.map_or(t.role, |c| c.role) as i32))
    }

    fn get_accState(&self, v: &VARIANT) -> Result<VARIANT> {
        self.with_child(v, |_, c| VARIANT::from(c.map_or(0, |c| c.state) as i32))
    }

    fn get_accHelp(&self, _: &VARIANT) -> Result<BSTR> {
        Err(S_FALSE.into())
    }

    fn get_accHelpTopic(&self, _: *mut BSTR, _: &VARIANT) -> Result<i32> {
        Err(S_FALSE.into())
    }

    fn get_accKeyboardShortcut(&self, _: &VARIANT) -> Result<BSTR> {
        Err(S_FALSE.into())
    }

    fn accFocus(&self) -> Result<VARIANT> {
        let tree = tree(self.source, self.hwnd);
        let focused = tree
            .children
            .iter()
            .position(|c| c.state & windows::Win32::UI::WindowsAndMessaging::STATE_SYSTEM_FOCUSED != 0);
        Ok(focused.map(child_variant).unwrap_or_default())
    }

    fn accSelection(&self) -> Result<VARIANT> {
        Ok(VARIANT::default())
    }

    fn get_accDefaultAction(&self, v: &VARIANT) -> Result<BSTR> {
        match self.with_child(v, |_, c| c.map(|c| c.action).unwrap_or(""))? {
            "" => Err(S_FALSE.into()),
            a => Ok(BSTR::from(a)),
        }
    }

    fn accSelect(&self, _: i32, _: &VARIANT) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn accLocation(&self, x: *mut i32, y: *mut i32, w: *mut i32, h: *mut i32, v: &VARIANT) -> Result<()> {
        let rect = self.with_child(v, |_, c| {
            c.map(|c| c.rect).unwrap_or_else(|| {
                let mut r = RECT::default();
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut r);
                }
                r
            })
        })?;
        if x.is_null() || y.is_null() || w.is_null() || h.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *x = rect.left;
            *y = rect.top;
            *w = rect.right - rect.left;
            *h = rect.bottom - rect.top;
        }
        Ok(())
    }

    fn accNavigate(&self, dir: i32, start: &VARIANT) -> Result<VARIANT> {
        let count = tree(self.source, self.hwnd).children.len();
        let Some(from) = which(start, count) else { return Err(E_INVALIDARG.into()) };
        let to = match (dir as u32, from) {
            (NAVDIR_FIRSTCHILD, None) if count > 0 => Some(0),
            (NAVDIR_LASTCHILD, None) if count > 0 => Some(count - 1),
            (NAVDIR_NEXT, Some(i)) if i + 1 < count => Some(i + 1),
            (NAVDIR_PREVIOUS, Some(i)) if i > 0 => Some(i - 1),
            _ => None,
        };
        match to {
            Some(i) => Ok(child_variant(i)),
            None => Err(S_FALSE.into()),
        }
    }

    fn accHitTest(&self, x: i32, y: i32) -> Result<VARIANT> {
        let tree = tree(self.source, self.hwnd);
        let inside = |r: &RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if let Some(i) = tree.children.iter().position(|c| inside(&c.rect)) {
            return Ok(child_variant(i));
        }
        let mut r = RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut r);
        }
        if inside(&r) { Ok(VARIANT::from(0i32)) } else { Err(S_FALSE.into()) }
    }

    fn accDoDefaultAction(&self, v: &VARIANT) -> Result<()> {
        let count = tree(self.source, self.hwnd).children.len();
        match which(v, count) {
            Some(Some(i)) => unsafe {
                PostMessageW(Some(self.hwnd), WM_APP_ACC_PRESS, WPARAM(i), LPARAM(0))?;
                Ok(())
            },
            _ => Err(E_INVALIDARG.into()),
        }
    }

    fn put_accName(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn put_accValue(&self, _: &VARIANT, _: &BSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}
