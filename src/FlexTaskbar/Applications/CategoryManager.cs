using System.Collections.ObjectModel;
using FlexTaskbar.Services;

namespace FlexTaskbar.Applications;

/// <summary>
/// Owns the category tree (persisted to categories.json) and the operations that
/// mutate it (spec Section 10: rename, new subcategory, move, delete). Deleting a
/// category never silently destroys data: subcategories are promoted to the deleted
/// category's parent, and apps that were in it become uncategorized rather than
/// disappearing.
/// </summary>
public sealed class CategoryManager
{
    private const string CacheFileName = "categories.json";

    public ObservableCollection<ApplicationCategory> Categories { get; } = new();

    /// <summary>Raised after any mutation so the UI can rebuild its category buttons/menus.</summary>
    public event Action? Changed;

    public void Initialize()
    {
        var loaded = ConfigurationService.Load<List<ApplicationCategory>>(CacheFileName);
        Categories.Clear();
        if (loaded is { Count: > 0 })
        {
            foreach (var category in loaded)
                Categories.Add(category);
        }

        // Also fired on re-initialize (e.g. after a Settings reset), not just first
        // load, so the UI reliably reflects whatever's on disk right now.
        Changed?.Invoke();
    }

    /// <summary>Replaces the whole tree (used by Settings import — spec Section 41).</summary>
    public void ReplaceAll(IEnumerable<ApplicationCategory> categories)
    {
        Categories.Clear();
        foreach (var category in categories)
            Categories.Add(category);

        SaveAndNotify();
    }

    public IEnumerable<ApplicationCategory> GetRootCategories() =>
        Categories.Where(c => c.ParentId is null)
            .OrderBy(c => c.SortOrder)
            .ThenBy(c => c.Name, StringComparer.CurrentCultureIgnoreCase);

    public IEnumerable<ApplicationCategory> GetChildren(string parentId) =>
        Categories.Where(c => c.ParentId == parentId)
            .OrderBy(c => c.SortOrder)
            .ThenBy(c => c.Name, StringComparer.CurrentCultureIgnoreCase);

    /// <summary>Flattens the tree depth-first with indentation depth, for "Move to Category" style pickers.</summary>
    public IEnumerable<(ApplicationCategory Category, int Depth)> GetAllFlattened()
    {
        foreach (var root in GetRootCategories())
            foreach (var item in FlattenFrom(root, 0))
                yield return item;
    }

    private IEnumerable<(ApplicationCategory, int)> FlattenFrom(ApplicationCategory category, int depth)
    {
        yield return (category, depth);
        foreach (var child in GetChildren(category.Id))
            foreach (var item in FlattenFrom(child, depth + 1))
                yield return item;
    }

    /// <summary>Applies an externally-computed SortOrder to whichever of this
    /// manager's categories appear in <paramref name="orderById"/> — used by the
    /// taskbar's center panel (see TaskbarWindow.ReorderCenterPanelItem) so root
    /// categories can share one ordering scale with pinned app shortcuts (round
    /// feedback: "make also the category can be reorder on the right with apps
    /// shortcut link"), letting a drag interleave a category among app shortcuts
    /// and vice versa, not just reorder within its own type.</summary>
    public void ApplyCenterPanelOrder(IReadOnlyDictionary<string, int> orderById)
    {
        foreach (var category in Categories)
        {
            if (orderById.TryGetValue(category.Id, out var order))
                category.SortOrder = order;
        }

        SaveAndNotify();
    }

    public ApplicationCategory AddCategory(string name, string? parentId)
    {
        var category = new ApplicationCategory
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = name,
            ParentId = parentId,
            SortOrder = Categories.Count(c => c.ParentId == parentId),
        };
        Categories.Add(category);
        SaveAndNotify();
        return category;
    }

    public void RenameCategory(string id, string newName)
    {
        var category = Categories.FirstOrDefault(c => c.Id == id);
        if (category is null || string.IsNullOrWhiteSpace(newName))
            return;

        category.Name = newName.Trim();
        SaveAndNotify();
    }

    /// <summary>Sets a category's icon glyph (spec Section 49 — "user can select a
    /// custom icon"). Currently a curated emoji set rather than arbitrary image
    /// files — see Menus/CategoryManagementWindow.xaml for the picker.</summary>
    public void SetIcon(string id, string glyph)
    {
        var category = Categories.FirstOrDefault(c => c.Id == id);
        if (category is null || string.IsNullOrWhiteSpace(glyph))
            return;

        category.IconGlyph = glyph;
        category.CustomIconPath = null; // picking an emoji glyph overrides any previously-chosen custom image icon
        SaveAndNotify();
    }

    /// <summary>Sets a category's icon to an app-owned optimized copy of a
    /// user-selected image file (round-2 feedback: custom category icons from
    /// disk — png/ico/svg). See <see cref="CategoryIconService.SaveOptimizedIcon"/>
    /// for the copy/optimize step; this method only wires the resulting path in.</summary>
    public void SetCustomIcon(string id, string sourceFilePath)
    {
        var category = Categories.FirstOrDefault(c => c.Id == id);
        if (category is null)
            return;

        category.CustomIconPath = CategoryIconService.SaveOptimizedIcon(sourceFilePath, category.Id);
        SaveAndNotify();
    }

    /// <summary>
    /// Deletes a category. Subcategories are promoted to the deleted category's
    /// parent (never orphaned or cascade-deleted) and apps assigned to it become
    /// uncategorized.
    /// </summary>
    public void DeleteCategory(string id, ApplicationManager applicationManager)
    {
        var category = Categories.FirstOrDefault(c => c.Id == id);
        if (category is null)
            return;

        foreach (var child in Categories.Where(c => c.ParentId == id).ToList())
            child.ParentId = category.ParentId;

        foreach (var app in applicationManager.Applications.Where(a => a.CategoryId == id))
            app.CategoryId = null;

        Categories.Remove(category);
        applicationManager.SaveCache();
        SaveAndNotify();
    }

    /// <summary>Reparents a category, refusing moves that would create a cycle.</summary>
    public bool MoveCategory(string id, string? newParentId)
    {
        if (id == newParentId)
            return false;

        var category = Categories.FirstOrDefault(c => c.Id == id);
        if (category is null || IsDescendantOf(newParentId, id))
            return false;

        category.ParentId = newParentId;
        SaveAndNotify();
        return true;
    }

    private bool IsDescendantOf(string? candidateId, string ancestorId)
    {
        var current = candidateId;
        while (current is not null)
        {
            if (current == ancestorId)
                return true;
            current = Categories.FirstOrDefault(c => c.Id == current)?.ParentId;
        }
        return false;
    }

    private void SaveAndNotify()
    {
        ConfigurationService.Save(CacheFileName, Categories.ToList());
        Changed?.Invoke();
    }
}
