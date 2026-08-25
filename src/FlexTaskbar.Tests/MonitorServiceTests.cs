using FlexTaskbar.Services;
using Xunit;

namespace FlexTaskbar.Tests;

/// <summary>
/// Exercises the real EnumDisplayMonitors/GetMonitorInfo P/Invoke calls (read-only —
/// safe to run anywhere, unlike TaskbarLayoutManager's AppBar registration which
/// changes shared desktop state and is intentionally not covered by an automated test).
/// </summary>
public class MonitorServiceTests
{
    [Fact]
    public void GetMonitors_ReturnsAtLeastOneMonitor()
    {
        var monitors = MonitorService.GetMonitors();

        Assert.NotEmpty(monitors);
    }

    [Fact]
    public void GetMonitors_ExactlyOnePrimary()
    {
        var monitors = MonitorService.GetMonitors();

        Assert.Single(monitors, m => m.IsPrimary);
    }

    [Fact]
    public void GetPrimary_ReturnsNonEmptyWorkArea()
    {
        var primary = MonitorService.GetPrimary();

        Assert.NotNull(primary);
        Assert.True(primary!.WorkArea.Width > 0);
        Assert.True(primary.WorkArea.Height > 0);
    }
}
