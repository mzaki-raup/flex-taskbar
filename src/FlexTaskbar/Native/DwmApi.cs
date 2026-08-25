using System.Runtime.InteropServices;

namespace FlexTaskbar.Native;

/// <summary>
/// Centralized P/Invoke surface for dwmapi.dll (spec Section 63). Currently used
/// only to detect DWM-cloaked windows — UWP/modern-app windows that pass every other
/// "real top-level window" check (visible, unowned, not a tool window) but are
/// actually hidden (e.g. suspended on another virtual desktop). Without this check,
/// EnumWindows-based taskbars famously show ghost entries for these; real Windows
/// filters them the same way.
/// </summary>
internal static class DwmApi
{
    public const int DWMWA_CLOAKED = 14;

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out int pvAttribute, int cbAttribute);
}
