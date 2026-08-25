using System.Windows;
using System.Windows.Interop;
using FlexTaskbar.Native;

namespace FlexTaskbar.Services;

/// <summary>
/// Registers a single system-wide hotkey (spec Section 17: "Win+Space or a
/// configurable hotkey") via RegisterHotKey, delivered through the owning window's
/// own HWND message loop — no separate message-only window needed since
/// TaskbarWindow already has one.
///
/// Win+Space is Windows' own default input-language-switch shortcut, so
/// registration can legitimately fail (another app, or the OS itself, already owns
/// it) — <see cref="Register"/> returns false rather than throwing, and the caller
/// decides how to surface that rather than this class pretending it always works.
/// </summary>
public sealed class GlobalHotkeyService : IDisposable
{
    private const int DefaultHotkeyId = 0x4A46; // arbitrary app-specific id ("JF" in hex, unlikely to collide)

    private readonly HwndSource _source;
    private readonly int _hotkeyId;
    private bool _registered;
    private bool _disposed;

    public event Action? HotkeyPressed;

    /// <param name="hotkeyId">
    /// Distinguishes multiple hotkeys registered on the same window (RegisterHotKey
    /// requires a unique id per HWND) — TaskbarWindow uses one instance for the
    /// launcher hotkey and a second, differently-ID'd instance for the recovery
    /// hotkey (Section 30).
    /// </param>
    public GlobalHotkeyService(Window window, int hotkeyId = DefaultHotkeyId)
    {
        _hotkeyId = hotkeyId;
        var handle = new WindowInteropHelper(window).Handle;
        _source = HwndSource.FromHwnd(handle)
            ?? throw new InvalidOperationException("Window must be initialized (have a real HWND) before registering hotkeys.");
        _source.AddHook(WndProc);
    }

    /// <summary>Attempts to register Win+Space. Returns false (does not throw) if the
    /// combination is already claimed by Windows or another app.</summary>
    public bool RegisterWinSpace() => Register(User32.MOD_WIN | User32.MOD_NOREPEAT, User32.VK_SPACE);

    /// <summary>
    /// Registers an arbitrary modifier+key combination (spec Section 17's
    /// "configurable hotkey"), replacing any combination currently registered by
    /// this instance. Returns false (does not throw) if the combination is already
    /// claimed by Windows or another app.
    /// </summary>
    public bool Register(uint modifiers, uint virtualKey)
    {
        if (_registered)
            User32.UnregisterHotKey(_source.Handle, _hotkeyId);

        _registered = User32.RegisterHotKey(_source.Handle, _hotkeyId, modifiers, virtualKey);
        return _registered;
    }

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg == User32.WM_HOTKEY && wParam.ToInt32() == _hotkeyId)
        {
            HotkeyPressed?.Invoke();
            handled = true;
        }

        return IntPtr.Zero;
    }

    public void Dispose()
    {
        if (_disposed)
            return;

        if (_registered)
            User32.UnregisterHotKey(_source.Handle, _hotkeyId);

        _source.RemoveHook(WndProc);
        _disposed = true;
    }
}
