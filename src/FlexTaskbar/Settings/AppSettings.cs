namespace FlexTaskbar.Settings;

/// <summary>
/// Persisted user preferences (spec Section 29). Serialized as plain strings/numbers
/// (not enums directly) so the JSON stays human-readable and doesn't break if enum
/// member order ever changes.
/// </summary>
public sealed class AppSettings
{
    public bool StartWithWindows { get; set; }
    public bool AutoHide { get; set; }
    public bool ReserveScreenSpace { get; set; }

    /// <summary>"Bottom" or "Top" — see Taskbar/TaskbarPosition.cs.</summary>
    public string Position { get; set; } = "Bottom";

    public double TaskbarHeight { get; set; } = 48.0; // matches the default Windows 11 taskbar height at 100% scaling

    /// <summary>One of the presets in Settings/HotkeyPresets.cs, e.g. "Win+Space".</summary>
    public string HotkeyPreset { get; set; } = "Win+Space";
}
