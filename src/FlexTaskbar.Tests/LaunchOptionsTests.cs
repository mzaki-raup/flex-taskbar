using FlexTaskbar;
using Xunit;

namespace FlexTaskbar.Tests;

public class LaunchOptionsTests
{
    [Fact]
    public void Parse_NoArgs_AllFlagsFalse()
    {
        var options = LaunchOptions.Parse(System.Array.Empty<string>());

        Assert.False(options.SafeMode);
        Assert.False(options.Disable);
        Assert.False(options.OpenSettings);
        Assert.False(options.Restart);
        Assert.False(options.Reset);
    }

    [Theory]
    [InlineData("--safe-mode")]
    [InlineData("--SAFE-MODE")]
    public void Parse_SafeModeFlag_IsCaseInsensitive(string flag)
    {
        var options = LaunchOptions.Parse(new[] { flag });

        Assert.True(options.SafeMode);
    }

    [Fact]
    public void Parse_DisableFlag_SetsDisableOnly()
    {
        var options = LaunchOptions.Parse(new[] { "--disable" });

        Assert.True(options.Disable);
        Assert.False(options.SafeMode);
    }

    [Fact]
    public void Parse_MultipleFlags_AllRecognized()
    {
        var options = LaunchOptions.Parse(new[] { "--settings", "--restart", "--reset" });

        Assert.True(options.OpenSettings);
        Assert.True(options.Restart);
        Assert.True(options.Reset);
    }
}
