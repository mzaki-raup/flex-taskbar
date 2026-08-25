using FlexTaskbar.Services;

namespace FlexTaskbar.Settings;

/// <summary>Loads/saves <see cref="AppSettings"/> via the shared atomic-write ConfigurationService.</summary>
public sealed class SettingsManager
{
    private const string FileName = "settings.json";

    public AppSettings Current { get; private set; } = new();

    public event Action? Changed;

    public void Load()
    {
        Current = ConfigurationService.Load<AppSettings>(FileName) ?? new AppSettings();
    }

    public void Save()
    {
        ConfigurationService.Save(FileName, Current);
        Changed?.Invoke();
    }

    /// <summary>Resets to defaults and persists immediately (used by Settings' "Reset Configuration").</summary>
    public void ResetToDefaults()
    {
        Current = new AppSettings();
        Save();
    }
}
