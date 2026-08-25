using System.Runtime.InteropServices;

namespace FlexTaskbar.Native;

/// <summary>
/// Centralized P/Invoke surface for the Windows AppBar API (spec Section 22/64) —
/// SHAppBarMessage is how a real taskbar reserves screen work area, instead of
/// merely being a topmost window that happens to sit at the screen edge (which is
/// all FlexTaskbar has done through Phase 5). Kept in its own file rather than
/// folded into Shell32.cs since it's a distinct, self-contained protocol (register →
/// query/negotiate position → set position → remove) with its own struct.
///
/// IMPORTANT: registering an AppBar changes work-area state for the whole desktop
/// (every other window's "maximized" bounds, Win+Arrow snapping, etc.), not just
/// FlexTaskbar's own window. Callers (see Taskbar/TaskbarLayoutManager.cs) must
/// guarantee ABM_REMOVE runs on every exit path, or the reservation can outlive the
/// process — see DEVELOPMENT.md for how this is handled and its known limits.
/// </summary>
internal static class AppBar
{
    public const uint ABM_NEW = 0x00000000;
    public const uint ABM_REMOVE = 0x00000001;
    public const uint ABM_QUERYPOS = 0x00000002;
    public const uint ABM_SETPOS = 0x00000003;
    public const uint ABM_GETSTATE = 0x00000004;
    public const uint ABM_ACTIVATE = 0x00000006;
    public const uint ABM_SETAUTOHIDEBAR = 0x00000008;
    public const uint ABM_WINDOWPOSCHANGED = 0x00000009;

    public const uint ABE_LEFT = 0;
    public const uint ABE_TOP = 1;
    public const uint ABE_RIGHT = 2;
    public const uint ABE_BOTTOM = 3;

    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uCallbackMessage;
        public uint uEdge;
        public RECT rc;
        public IntPtr lParam;
    }

    [DllImport("shell32.dll")]
    public static extern IntPtr SHAppBarMessage(uint dwMessage, ref APPBARDATA pData);
}
