using System.Runtime.InteropServices;
using System.Windows;
using FlexTaskbar.Native;

namespace FlexTaskbar.Services;

/// <summary>A physical display, in device pixels (spec Section 19).</summary>
public sealed record MonitorInfo(IntPtr Handle, Rect Bounds, Rect WorkArea, bool IsPrimary);

/// <summary>
/// Enumerates connected monitors via EnumDisplayMonitors/GetMonitorInfo (spec
/// Section 64). Read-only — this never changes anything on screen, unlike
/// TaskbarLayoutManager's AppBar registration, so it's safe to call freely.
/// </summary>
public static class MonitorService
{
    public static List<MonitorInfo> GetMonitors()
    {
        var monitors = new List<MonitorInfo>();

        User32.EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr hMonitor, IntPtr _, ref RECT _, IntPtr _) =>
        {
            var info = new User32.MONITORINFO { cbSize = (uint)Marshal.SizeOf<User32.MONITORINFO>() };
            if (User32.GetMonitorInfo(hMonitor, ref info))
            {
                monitors.Add(new MonitorInfo(
                    hMonitor,
                    ToRect(info.rcMonitor),
                    ToRect(info.rcWork),
                    (info.dwFlags & User32.MONITORINFOF_PRIMARY) != 0));
            }

            return true;
        }, IntPtr.Zero);

        return monitors;
    }

    public static MonitorInfo? GetPrimary() => GetMonitors().FirstOrDefault(m => m.IsPrimary);

    private static Rect ToRect(RECT r) => new(r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top);
}
