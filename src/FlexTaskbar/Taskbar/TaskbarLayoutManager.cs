using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using FlexTaskbar.Native;
using FlexTaskbar.Services;

namespace FlexTaskbar.Taskbar;

/// <summary>
/// Registers <see cref="TaskbarWindow"/> as a real Windows AppBar (spec Section 22),
/// which reserves screen work area the way the actual Windows taskbar does — as
/// opposed to Phases 1–5's approach of just being a topmost window that happens to
/// sit at the screen edge without anyone else's layout being affected.
///
/// <b>Disabled by default and must be explicitly enabled.</b> Registering an AppBar
/// changes shared desktop state (every other window's "maximized" bounds, Win+Arrow
/// snapping, etc.) — this is qualitatively different from anything earlier phases
/// did, and matches spec Section 2's insistence that FlexTaskbar "must run safely
/// alongside the existing Windows taskbar" by default.
///
/// Registration is only ever attempted after the window has a real HWND, and
/// <see cref="Unregister"/> must run on every exit path (graceful close, and — best
/// effort — process exit) or the reservation can outlive the process until Explorer
/// restarts. See DEVELOPMENT.md for how this was verified and its known limits.
/// </summary>
public sealed class TaskbarLayoutManager : IDisposable
{
    private readonly Window _window;
    private bool _registered;

    // Captured once, right before the first-ever reservation — see Register().
    // ComputeRequestedRect anchors to THIS, never to a fresh MonitorService query,
    // because the live/current work area already reflects our own prior
    // reservation once registered. Re-querying it on every ApplyPosition() call
    // (as this used to do) fed that already-shrunk value back in as the new
    // baseline each time, so every periodic re-application (the 20s AppBar
    // health-check timer, see TaskbarWindow) claimed an *additional* sliver of
    // screen on top of the last one — a compounding feedback loop, not a stable
    // reservation. Symptom reported live: "it gradually shrinks the other app
    // windows... it not stay on the bottom."
    private Rect? _baselineWorkArea;

    public TaskbarPosition Position { get; set; } = TaskbarPosition.Bottom;
    public double BarThickness { get; set; } = 40.0;
    public bool IsRegistered => _registered;

    public TaskbarLayoutManager(Window window)
    {
        _window = window;
    }

    /// <summary>Registers with the shell and immediately claims a position. Returns
    /// false if the window doesn't have an HWND yet (call after Loaded/SourceInitialized).</summary>
    public bool Register()
    {
        if (_registered)
            return true;

        // Fresh baseline: this is a genuine first-time (or post-Unregister)
        // registration, so the current work area is guaranteed to reflect every
        // *other* AppBar but not ours yet — see the field's doc comment.
        return RegisterCore(captureBaseline: true);
    }

    /// <summary>
    /// Re-claims the AppBar registration after Explorer restarts (spec Section 46).
    /// Explorer itself owns the registered-AppBar list; when it restarts, that list
    /// is gone, so our own <see cref="_registered"/> flag would otherwise be lying —
    /// we'd think we still have a reservation when the shell has forgotten about it.
    /// Called both when TaskbarWindow receives the well-known "TaskbarCreated"
    /// message and unconditionally on a periodic health-check timer (see
    /// TaskbarWindow) — there's no official "is my hwnd still a registered AppBar?"
    /// query, so the timer re-asserts on a schedule rather than only reacting to a
    /// detected restart. A no-op if we weren't registered in the first place.
    /// </summary>
    public void HandleShellRestarted()
    {
        if (!_registered)
            return;

        // Deliberately does NOT re-capture the baseline. This runs unconditionally
        // every 20 seconds regardless of whether Explorer actually restarted —
        // treating every tick as "capture a fresh baseline" reintroduced the exact
        // compounding-shrink bug the _baselineWorkArea field exists to prevent,
        // just through this path instead of ApplyPosition()'s. The cached baseline
        // from the original Register() call remains correct here: even across a
        // real Explorer restart, the *external* work area (real taskbar, other
        // AppBars) it captured hasn't changed — only our own registration with
        // Explorer was forgotten, which is exactly what re-issuing ABM_NEW fixes.
        _registered = false;
        RegisterCore(captureBaseline: false);
    }

    private bool RegisterCore(bool captureBaseline)
    {
        var hwnd = new WindowInteropHelper(_window).Handle;
        if (hwnd == IntPtr.Zero)
            return false;

        if (captureBaseline || _baselineWorkArea is null)
            _baselineWorkArea = MonitorService.GetPrimary()?.WorkArea;

        var data = new AppBar.APPBARDATA
        {
            cbSize = (uint)Marshal.SizeOf<AppBar.APPBARDATA>(),
            hWnd = hwnd,
        };

        AppBar.SHAppBarMessage(AppBar.ABM_NEW, ref data);
        _registered = true;

        ApplyPosition();
        return true;
    }

    /// <summary>Re-queries and re-claims the reserved rectangle — call after
    /// changing <see cref="Position"/>/<see cref="BarThickness"/>, or when the taskbar's
    /// expanded/collapsed state changes (auto-hide), so the reservation stays in sync
    /// with what's actually on screen.</summary>
    public void ApplyPosition()
    {
        if (!_registered)
            return;

        // Anchor to the work area captured once in Register() — NOT a fresh
        // MonitorService query. See _baselineWorkArea's doc comment: querying
        // "the current work area" here used to mean "the work area including our
        // own already-registered reservation," so every periodic re-application
        // shrank it further, compounding without bound. Falls back to a fresh
        // query only if something odd left the baseline unset (e.g. Register()
        // couldn't resolve a monitor at the time) rather than silently no-op-ing.
        var workArea = _baselineWorkArea ?? MonitorService.GetPrimary()?.WorkArea;
        if (workArea is null)
            return;

        var hwnd = new WindowInteropHelper(_window).Handle;
        // Anchor to the work area edge, not the raw monitor edge — the real
        // Windows taskbar already occupies the physical screen edge, so
        // requesting to dock exactly there produces a degenerate/inverted rect once
        // the shell's ABM_QUERYPOS negotiation shrinks our edge to avoid overlapping
        // it (caught live: requesting Bounds.Bottom on this machine returned a rect
        // with Bottom < Top). Docking to the work-area edge instead keeps FlexTaskbar
        // sitting just above the real taskbar, matching the same "safe alongside"
        // positioning Phases 1–5 already use without AppBar.
        var requestedRect = ComputeRequestedRect(workArea.Value);
        var data = new AppBar.APPBARDATA
        {
            cbSize = (uint)Marshal.SizeOf<AppBar.APPBARDATA>(),
            hWnd = hwnd,
            uEdge = Position == TaskbarPosition.Top ? AppBar.ABE_TOP : AppBar.ABE_BOTTOM,
            rc = requestedRect,
        };

        // ABM_QUERYPOS lets the shell adjust rc (e.g. to avoid overlapping another
        // AppBar); ABM_SETPOS then commits whatever rect comes back from that query,
        // not our original request — this two-step negotiation is the documented
        // AppBar protocol, skipping QUERYPOS is a common bug in naive implementations.
        // (Verified live: without the WorkArea anchoring above, QUERYPOS/SETPOS
        // returned a rect with Bottom < Top after negotiating around the real
        // taskbar's already-claimed screen edge — a genuinely inverted rectangle,
        // not a hypothetical edge case.)
        AppBar.SHAppBarMessage(AppBar.ABM_QUERYPOS, ref data);
        AppBar.SHAppBarMessage(AppBar.ABM_SETPOS, ref data);

        // GetMonitorInfo/SHAppBarMessage both work in *physical* pixels; WPF's
        // Window.Left/Top/Width/Height are in DIPs (96-DPI logical units). Writing
        // the physical rect straight into those properties is wrong on any display
        // that isn't running at 100% scaling. VisualTreeHelper.GetDpi requires the
        // window to already have a HWND/visual, true here since Register() only
        // calls this after the window is Loaded.
        var dpi = System.Windows.Media.VisualTreeHelper.GetDpi(_window);
        _window.Left = data.rc.Left / dpi.DpiScaleX;
        _window.Top = data.rc.Top / dpi.DpiScaleY;
        _window.Width = (data.rc.Right - data.rc.Left) / dpi.DpiScaleX;
        _window.Height = (data.rc.Bottom - data.rc.Top) / dpi.DpiScaleY;
    }

    private RECT ComputeRequestedRect(Rect monitorBounds)
    {
        var left = (int)monitorBounds.Left;
        var right = (int)monitorBounds.Right;

        // monitorBounds is in physical pixels (from MonitorService/GetMonitorInfo);
        // BarThickness is in DIPs (it's shared with the pre-AppBar DIP-based sizing
        // in TaskbarWindow), so it must be scaled up to physical pixels here or the
        // reserved strip ends up the wrong size on any non-100%-scaled display —
        // same root cause as the Left/Top/Width/Height conversion in ApplyPosition.
        var dpi = System.Windows.Media.VisualTreeHelper.GetDpi(_window);
        var thicknessPhysical = BarThickness * dpi.DpiScaleY;

        return Position == TaskbarPosition.Bottom
            ? new RECT { Left = left, Right = right, Bottom = (int)monitorBounds.Bottom, Top = (int)(monitorBounds.Bottom - thicknessPhysical) }
            : new RECT { Left = left, Right = right, Top = (int)monitorBounds.Top, Bottom = (int)(monitorBounds.Top + thicknessPhysical) };
    }

    public void Unregister()
    {
        if (!_registered)
            return;

        var hwnd = new WindowInteropHelper(_window).Handle;
        var data = new AppBar.APPBARDATA
        {
            cbSize = (uint)Marshal.SizeOf<AppBar.APPBARDATA>(),
            hWnd = hwnd,
        };

        AppBar.SHAppBarMessage(AppBar.ABM_REMOVE, ref data);
        _registered = false;
        _baselineWorkArea = null; // next Register() must re-capture a clean baseline, not reuse a stale one
    }

    public void Dispose() => Unregister();
}
