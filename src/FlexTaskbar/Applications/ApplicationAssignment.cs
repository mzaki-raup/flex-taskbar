namespace FlexTaskbar.Applications;

/// <summary>
/// The user-customization subset of an <see cref="ApplicationEntry"/> — category,
/// pin, favorite — keyed by the entry's stable Id. This is what gets exported/
/// imported (spec Section 41), not the full discovered app list, since the app list
/// itself is always regenerable by rescanning.
/// </summary>
public sealed class ApplicationAssignment
{
    public required string ApplicationId { get; set; }
    public string? CategoryId { get; set; }
    public bool IsPinned { get; set; }
    public bool IsFavorite { get; set; }

    /// <summary>Position among pinned taskbar shortcuts — see
    /// <see cref="ApplicationEntry.SortOrder"/>. Included so a drag-reordered pin
    /// order survives export/import, not just a restart.</summary>
    public int SortOrder { get; set; }

    /// <summary>Position among sibling apps within the same category — see
    /// <see cref="ApplicationEntry.CategorySortOrder"/>. Same reasoning as
    /// <see cref="SortOrder"/>: a drag-reordered category app list should survive
    /// export/import too.</summary>
    public int CategorySortOrder { get; set; }
}
