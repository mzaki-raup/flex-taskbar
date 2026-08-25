using FlexTaskbar.Applications;
using Xunit;

namespace FlexTaskbar.Tests;

public class ApplicationSearchTests
{
    private static ApplicationEntry MakeApp(string id, string name, string exePath, string? categoryId = null) =>
        new()
        {
            Id = id,
            Name = name,
            ExecutablePath = exePath,
            CategoryId = categoryId,
        };

    [Fact]
    public void Search_EmptyQuery_ReturnsNoResults()
    {
        var apps = new[] { MakeApp("1", "Visual Studio Code", @"C:\code.exe") };
        var categoryManager = new CategoryManager();

        var results = ApplicationSearch.Search("", apps, categoryManager);

        Assert.Empty(results);
    }

    [Fact]
    public void Search_MatchesNameSubstring_CaseInsensitive()
    {
        var apps = new[]
        {
            MakeApp("1", "Visual Studio Code", @"C:\Code.exe"),
            MakeApp("2", "OpenCode", @"C:\opencode.exe"),
            MakeApp("3", "Notepad", @"C:\notepad.exe"),
        };
        var categoryManager = new CategoryManager();

        var results = ApplicationSearch.Search("CODE", apps, categoryManager);

        Assert.Equal(2, results.Count);
        Assert.Contains(results, a => a.Id == "1");
        Assert.Contains(results, a => a.Id == "2");
    }

    [Fact]
    public void Search_PrefixMatch_SortsBeforeSubstringMatch()
    {
        var apps = new[]
        {
            MakeApp("1", "My Blender Addon", @"C:\a.exe"),
            MakeApp("2", "Blender", @"C:\b.exe"),
        };
        var categoryManager = new CategoryManager();

        var results = ApplicationSearch.Search("Blender", apps, categoryManager);

        Assert.Equal("2", results[0].Id); // starts-with "Blender" ranks first
        Assert.Equal("1", results[1].Id);
    }

    [Fact]
    public void Search_MatchesCategoryName_IncludesAppsInThatCategory()
    {
        var apps = new[]
        {
            MakeApp("1", "Ollama", @"C:\ollama.exe", categoryId: "cat-ai"),
            MakeApp("2", "Notepad", @"C:\notepad.exe"),
        };
        var categoryManager = new CategoryManager();
        categoryManager.Categories.Add(new ApplicationCategory { Id = "cat-ai", Name = "AI", ParentId = null });

        var results = ApplicationSearch.Search("AI", apps, categoryManager);

        Assert.Single(results);
        Assert.Equal("1", results[0].Id);
    }

    [Fact]
    public void Search_MatchesCategoryName_IncludesDescendantCategoryApps()
    {
        var apps = new[]
        {
            MakeApp("1", "ComfyUI", @"C:\comfy.exe", categoryId: "cat-image"),
        };
        var categoryManager = new CategoryManager();
        categoryManager.Categories.Add(new ApplicationCategory { Id = "cat-ai", Name = "AI", ParentId = null });
        categoryManager.Categories.Add(new ApplicationCategory { Id = "cat-image", Name = "Image Generation", ParentId = "cat-ai" });

        var results = ApplicationSearch.Search("AI", apps, categoryManager);

        Assert.Single(results);
        Assert.Equal("1", results[0].Id);
    }

    [Fact]
    public void Search_NoMatches_ReturnsEmptyList()
    {
        var apps = new[] { MakeApp("1", "Notepad", @"C:\notepad.exe") };
        var categoryManager = new CategoryManager();

        var results = ApplicationSearch.Search("zzz-nonexistent", apps, categoryManager);

        Assert.Empty(results);
    }
}
