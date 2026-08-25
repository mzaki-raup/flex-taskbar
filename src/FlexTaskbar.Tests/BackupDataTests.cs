using System.Text.Json;
using FlexTaskbar.Applications;
using FlexTaskbar.Settings;
using Xunit;

namespace FlexTaskbar.Tests;

public class BackupDataTests
{
    [Fact]
    public void SerializeThenDeserialize_RoundTripsCategoriesAndAssignments()
    {
        var backup = new BackupData
        {
            Categories = new List<ApplicationCategory>
            {
                new() { Id = "cat1", Name = "Development", ParentId = null, SortOrder = 0 },
                new() { Id = "cat2", Name = "IDEs", ParentId = "cat1", SortOrder = 0 },
            },
            AppAssignments = new List<ApplicationAssignment>
            {
                new() { ApplicationId = "app1", CategoryId = "cat2", IsPinned = true, IsFavorite = false },
            },
            Settings = new AppSettings { TaskbarHeight = 48, Position = "Top" },
        };

        var json = JsonSerializer.Serialize(backup);
        var restored = JsonSerializer.Deserialize<BackupData>(json);

        Assert.NotNull(restored);
        Assert.Equal(2, restored!.Categories.Count);
        Assert.Equal("IDEs", restored.Categories[1].Name);
        Assert.Equal("cat1", restored.Categories[1].ParentId);
        Assert.Single(restored.AppAssignments);
        Assert.True(restored.AppAssignments[0].IsPinned);
        Assert.Equal(48, restored.Settings!.TaskbarHeight);
        Assert.Equal("Top", restored.Settings.Position);
    }

    [Fact]
    public void Deserialize_EmptyObject_ProducesEmptyDefaults()
    {
        var restored = JsonSerializer.Deserialize<BackupData>("{}");

        Assert.NotNull(restored);
        Assert.Empty(restored!.Categories);
        Assert.Empty(restored.AppAssignments);
        Assert.Null(restored.Settings);
    }
}
