using System.IO;
using FlexTaskbar.Services;
using FlexTaskbar.Utilities;
using Xunit;

namespace FlexTaskbar.Tests;

public class ConfigurationServiceTests
{
    private sealed class SamplePayload
    {
        public string? Name { get; set; }
        public int Count { get; set; }
    }

    private static string FilePath(string fileName) => Path.Combine(AppPaths.RoamingRoot, fileName);

    [Fact]
    public void SaveThenLoad_RoundTripsData()
    {
        var fileName = $"test-{Guid.NewGuid():N}.json";
        try
        {
            var payload = new SamplePayload { Name = "FlexTaskbar", Count = 42 };

            ConfigurationService.Save(fileName, payload);
            var loaded = ConfigurationService.Load<SamplePayload>(fileName);

            Assert.NotNull(loaded);
            Assert.Equal("FlexTaskbar", loaded!.Name);
            Assert.Equal(42, loaded.Count);
        }
        finally
        {
            File.Delete(FilePath(fileName));
            File.Delete(FilePath(fileName) + ".bak");
        }
    }

    [Fact]
    public void Load_MissingFile_ReturnsNull()
    {
        var result = ConfigurationService.Load<SamplePayload>($"missing-{Guid.NewGuid():N}.json");

        Assert.Null(result);
    }

    [Fact]
    public void Load_CorruptPrimary_FallsBackToBackup()
    {
        var fileName = $"test-{Guid.NewGuid():N}.json";
        var path = FilePath(fileName);
        try
        {
            // First save establishes a good backup on the *second* save.
            ConfigurationService.Save(fileName, new SamplePayload { Name = "good", Count = 1 });
            ConfigurationService.Save(fileName, new SamplePayload { Name = "good", Count = 1 });

            File.WriteAllText(path, "{ not valid json");

            var loaded = ConfigurationService.Load<SamplePayload>(fileName);

            Assert.NotNull(loaded);
            Assert.Equal("good", loaded!.Name);
        }
        finally
        {
            File.Delete(path);
            File.Delete(path + ".bak");
        }
    }
}
