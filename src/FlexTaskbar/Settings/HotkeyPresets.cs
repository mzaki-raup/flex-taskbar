using FlexTaskbar.Native;

namespace FlexTaskbar.Settings;

/// <summary>
/// A small curated set of launcher hotkey combinations (spec Section 17:
/// "configurable hotkey"). Win+Space is the default but frequently loses the
/// OS-level claim to Windows' own input-language switcher (confirmed on the dev
/// machine during Phase 5) — these presets give the user a way out without building
/// a full "press any key" capture control.
/// </summary>
public static class HotkeyPresets
{
    public static readonly IReadOnlyList<string> Names = new[]
    {
        "Win+Space",
        "Ctrl+Alt+Space",
        "Ctrl+Shift+Space",
        "Ctrl+Alt+K",
    };

    public static (uint Modifiers, uint VirtualKey) Resolve(string presetName) => presetName switch
    {
        "Ctrl+Alt+Space" => (User32.MOD_CONTROL | User32.MOD_ALT | User32.MOD_NOREPEAT, User32.VK_SPACE),
        "Ctrl+Shift+Space" => (User32.MOD_CONTROL | User32.MOD_SHIFT | User32.MOD_NOREPEAT, User32.VK_SPACE),
        "Ctrl+Alt+K" => (User32.MOD_CONTROL | User32.MOD_ALT | User32.MOD_NOREPEAT, User32.VK_K),
        _ => (User32.MOD_WIN | User32.MOD_NOREPEAT, User32.VK_SPACE), // "Win+Space" and any unrecognized value
    };
}
