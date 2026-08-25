using FlexTaskbar.Settings;
using Xunit;

namespace FlexTaskbar.Tests;

public class HotkeyPresetsTests
{
    [Fact]
    public void Resolve_UnknownPreset_FallsBackToWinSpaceCombo()
    {
        var winSpace = HotkeyPresets.Resolve("Win+Space");
        var unknown = HotkeyPresets.Resolve("NotARealPreset");

        Assert.Equal(winSpace, unknown);
    }

    [Fact]
    public void AllNamedPresets_ResolveToDistinctCombos()
    {
        var combos = HotkeyPresets.Names.Select(HotkeyPresets.Resolve).ToList();

        Assert.Equal(combos.Count, combos.Distinct().Count());
    }

    [Fact]
    public void Resolve_SameInputTwice_ReturnsSameCombo()
    {
        Assert.Equal(HotkeyPresets.Resolve("Ctrl+Alt+K"), HotkeyPresets.Resolve("Ctrl+Alt+K"));
    }
}
