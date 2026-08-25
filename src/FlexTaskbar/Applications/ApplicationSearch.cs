using System.IO;

namespace FlexTaskbar.Applications;

/// <summary>
/// Search matching for the launcher (spec Section 17/51): app name, executable file
/// name, and — the part easy to forget — category name. Searching "AI" should return
/// everything filed under the "AI" category (and its subcategories), not just an app
/// literally named "AI".
/// </summary>
public static class ApplicationSearch
{
    private const int MaxResults = 50;

    public static List<ApplicationEntry> Search(string query, IEnumerable<ApplicationEntry> applications, CategoryManager categoryManager)
    {
        query = query.Trim();
        if (query.Length == 0)
            return new List<ApplicationEntry>();

        var matchingCategoryIds = ExpandToDescendants(
            categoryManager,
            categoryManager.Categories.Where(c => c.Name.Contains(query, StringComparison.OrdinalIgnoreCase)).Select(c => c.Id));

        return applications
            .Where(a =>
                a.Name.Contains(query, StringComparison.OrdinalIgnoreCase) ||
                Path.GetFileName(a.ExecutablePath).Contains(query, StringComparison.OrdinalIgnoreCase) ||
                (a.CategoryId is not null && matchingCategoryIds.Contains(a.CategoryId)))
            .OrderBy(a => !a.Name.StartsWith(query, StringComparison.OrdinalIgnoreCase)) // prefix matches first
            .ThenBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase)
            .Take(MaxResults)
            .ToList();
    }

    private static HashSet<string> ExpandToDescendants(CategoryManager categoryManager, IEnumerable<string> seedIds)
    {
        var ids = new HashSet<string>(seedIds);

        bool changed = true;
        while (changed)
        {
            changed = false;
            foreach (var category in categoryManager.Categories)
            {
                if (category.ParentId is not null && ids.Contains(category.ParentId) && ids.Add(category.Id))
                    changed = true;
            }
        }

        return ids;
    }
}
