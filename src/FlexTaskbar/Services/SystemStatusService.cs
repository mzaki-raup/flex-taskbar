namespace FlexTaskbar.Services;

/// <summary>
/// Real system status readouts for the taskbar's tray-style indicator area (spec
/// Section 14). Battery uses <see cref="System.Windows.Forms.SystemInformation.PowerStatus"/>
/// (already available since the tray icon pulls in WinForms) rather than new
/// P/Invoke. Network and volume indicators were removed (round feedback: "remove
/// also network and volume mixer icon on the far right").
/// </summary>
public static class SystemStatusService
{
    /// <summary>Returns null if there's no battery (desktop machine) — callers
    /// should hide the indicator entirely rather than show a fake/inapplicable one.</summary>
    public static (int Percent, bool IsCharging)? GetBatteryStatus()
    {
        var status = System.Windows.Forms.SystemInformation.PowerStatus;
        if (status.BatteryChargeStatus.HasFlag(System.Windows.Forms.BatteryChargeStatus.NoSystemBattery))
            return null;

        var percent = (int)Math.Round(status.BatteryLifePercent * 100);
        var charging = status.PowerLineStatus == System.Windows.Forms.PowerLineStatus.Online;
        return (percent, charging);
    }
}
