using System.Runtime.InteropServices;
using System.Text;

namespace FlexTaskbar.Native;

/// <summary>
/// Centralized P/Invoke surface for user32.dll (spec Section 63). AppBar-related
/// calls (SHAppBarMessage) are added in Phase 6 when work-area reservation lands.
///
/// Note: GetWindowLongPtr is declared as-is (no 32-bit GetWindowLong fallback) since
/// the project only targets x64/arm64 (see FlexTaskbar.csproj Platforms) — on those,
/// GetWindowLongPtrW is the correct, pointer-width-safe export.
/// </summary>
internal static class User32
{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    public delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);

    public const uint MONITORINFOF_PRIMARY = 0x00000001;

    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO
    {
        public uint cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    /// <summary>
    /// Callback signature for SetWinEventHook. The delegate instance passed in must be
    /// kept alive by the caller for the lifetime of the hook — the CLR has no other
    /// reference to it, so letting it get GC'd silently breaks the hook (a classic
    /// P/Invoke pitfall). See WindowWatcher, which stores it in a field.
    /// </summary>
    public delegate void WinEventDelegate(
        IntPtr hWinEventHook, uint eventType, IntPtr hwnd,
        int idObject, int idChild, uint dwEventThread, uint dwmsEventTime);

    // --- ShowWindow / SetForegroundWindow commands ---
    public const int SW_MAXIMIZE = 3;
    public const int SW_MINIMIZE = 6;
    public const int SW_RESTORE = 9;

    // --- GetWindow relationships ---
    public const uint GW_OWNER = 4;

    // --- GetWindowLongPtr indices / extended styles ---
    public const int GWL_EXSTYLE = -20;
    public const long WS_EX_TOOLWINDOW = 0x00000080;
    public const long WS_EX_APPWINDOW = 0x00040000;

    // --- Window messages ---
    public const uint WM_CLOSE = 0x0010;

    // --- WinEvent hook: event IDs actually used by WindowWatcher ---
    public const uint EVENT_SYSTEM_FOREGROUND = 0x0003;
    public const uint EVENT_SYSTEM_MINIMIZESTART = 0x0016;
    public const uint EVENT_SYSTEM_MINIMIZEEND = 0x0017;
    public const uint EVENT_OBJECT_CREATE = 0x8000;
    public const uint EVENT_OBJECT_DESTROY = 0x8001;
    public const uint EVENT_OBJECT_SHOW = 0x8002;
    public const uint EVENT_OBJECT_HIDE = 0x8003;

    // --- WinEvent hook flags ---
    public const uint WINEVENT_OUTOFCONTEXT = 0x0000;
    public const uint WINEVENT_SKIPOWNPROCESS = 0x0002;

    public const int OBJID_WINDOW = 0;

    // --- RegisterHotKey modifiers/message ---
    public const uint MOD_ALT = 0x0001;
    public const uint MOD_CONTROL = 0x0002;
    public const uint MOD_SHIFT = 0x0004;
    public const uint MOD_WIN = 0x0008;
    public const uint MOD_NOREPEAT = 0x4000;
    public const uint VK_SPACE = 0x20;
    public const uint VK_K = 0x4B; // letter virtual-key codes equal their ASCII uppercase value
    public const uint VK_F12 = 0x7B;
    public const int WM_HOTKEY = 0x0312;

    // --- LockWorkStation / system power ---
    // (ExitWindowsEx intentionally not used for shutdown/restart — see
    // Services/PowerActionService.cs, which shells out to shutdown.exe instead so it
    // never needs SE_SHUTDOWN privilege elevation.)

    /// <summary>
    /// Frees an icon handle returned by SHGetFileInfo. Without this, repeated icon
    /// extraction leaks GDI handles — see ApplicationIconService.
    /// </summary>
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool DestroyIcon(IntPtr hIcon);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextLength(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    public static extern IntPtr GetWindow(IntPtr hWnd, uint uCmd);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    public static extern IntPtr GetWindowLongPtr(IntPtr hWnd, int nIndex);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    public static extern IntPtr SetWindowLongPtr(IntPtr hWnd, int nIndex, IntPtr dwNewLong);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    // --- SetWindowPos: used to keep the taskbar reliably pinned above other
    // topmost windows. WPF's Window.Topmost="True" only asserts the topmost band
    // once; another app setting itself topmost afterward (or various z-order churn)
    // can still end up drawn above us. Periodically re-issuing HWND_TOPMOST is the
    // standard fix real taskbar-replacement tools use.
    public static readonly IntPtr HWND_TOPMOST = new(-1);
    public const uint SWP_NOMOVE = 0x0002;
    public const uint SWP_NOSIZE = 0x0001;
    public const uint SWP_NOACTIVATE = 0x0010;

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int x, int y, int cx, int cy, uint uFlags);

    [DllImport("user32.dll")]
    public static extern IntPtr SetWinEventHook(
        uint eventMin, uint eventMax, IntPtr hmodWinEventProc,
        WinEventDelegate lpfnWinEventProc, uint idProcess, uint idThread, uint dwFlags);

    [DllImport("user32.dll")]
    public static extern bool UnhookWinEvent(IntPtr hWinEventHook);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool UnregisterHotKey(IntPtr hWnd, int id);

    /// <summary>Locks the interactive session (Win+L equivalent) — needs no special
    /// privilege, unlike shutdown/restart.</summary>
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool LockWorkStation();

    [DllImport("user32.dll")]
    public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumProc lpfnEnum, IntPtr dwData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);

    /// <summary>
    /// Registers a system-wide message ID. Explorer broadcasts the well-known
    /// "TaskbarCreated" message to every top-level window when it (re)starts — this
    /// is exactly how tray icons know to re-add themselves after an Explorer crash
    /// or manual restart, and TaskbarLayoutManager listens for it the same way to
    /// re-claim a lost AppBar reservation (spec Section 46 — Explorer-restart
    /// tolerance).
    /// </summary>
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern uint RegisterWindowMessage(string lpString);

    /// <summary>Locates the real Windows taskbar's top-level window ("Shell_TrayWnd")
    /// — used by TrayMirrorService to reach the real tray icon strip for MSAA
    /// right-click forwarding.</summary>
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr FindWindow(string? lpClassName, string? lpWindowName);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr hWndParent, EnumWindowsProc lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    // --- Synthetic right-click forwarding (TrayMirrorService): used only on a
    // direct user right-click of a mirrored tray icon, to trigger that icon's own
    // real Shell_NotifyIcon context menu — there's no other way to show it, since
    // the menu is owned and drawn by the icon's own app/Explorer, not by us.
    // SendInput (not the legacy mouse_event) since it's the API Microsoft actually
    // recommends for synthetic input and is what reliably registers with modern,
    // XAML-island-hosted shell controls like the tray's overflow flyout.
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT
    {
        public int dx;
        public int dy;
        public uint mouseData;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT
    {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct INPUT
    {
        [FieldOffset(0)] public uint type;
        [FieldOffset(8)] public MOUSEINPUT mi;
        [FieldOffset(8)] public KEYBDINPUT ki;
    }

    public const uint INPUT_MOUSE = 0;
    public const uint INPUT_KEYBOARD = 1;
    public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    public const uint MOUSEEVENTF_LEFTUP = 0x0004;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const ushort VK_APPS = 0x5D; // the "Menu" key — the standard keyboard equivalent of a right-click on the focused control
    public const ushort VK_SHIFT = 0x10;
    public const ushort VK_F10 = 0x79; // Shift+F10 is the other standard "show context menu" shortcut

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    /// <summary>Screen-coordinate bounds of a specific window — used only to open/
    /// close the tray's "^" overflow chevron (a toggle button, not something with
    /// a "show menu" action to select-and-Menu-key into) via a real synthetic
    /// click. Individual tray icons are never located this way — see
    /// TrayMirrorService's class doc for why coordinate-based icon clicking was
    /// abandoned.</summary>
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
}
