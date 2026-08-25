namespace FlexTaskbar.Applications;

/// <summary>
/// A user-defined category/subcategory (spec Section 5). Nesting is expressed via
/// <see cref="ParentId"/> rather than an owned collection so the flat list serializes
/// simply and categories can be reparented (drag/drop) without rewriting a tree.
/// Nesting depth is unbounded — <see cref="CategoryManager"/> just keeps walking
/// ParentId links, so "3+ levels" (Section 5) is a consequence of the data model, not
/// a special case.
/// </summary>
public sealed class ApplicationCategory
{
    public required string Id { get; init; }
    public required string Name { get; set; }
    public string? ParentId { get; set; }
    public string IconGlyph { get; set; } = "\U0001F4C1"; // 📁 default folder glyph, used as a fallback when CustomIconPath is unset

    /// <summary>Position among sibling categories (same ParentId). For root
    /// categories specifically, this shares its ordering scale with pinned app
    /// shortcuts' own SortOrder (round feedback: "make also the category can be
    /// reorder on the right with apps shortcut link") so the taskbar's center
    /// panel can interleave the two by drag-and-drop — see
    /// <see cref="ApplicationManager.ApplyCenterPanelOrder"/>.</summary>
    public int SortOrder { get; set; }

    /// <summary>
    /// Path to an app-owned, optimized PNG copy of a user-selected icon file
    /// (spec round-2 feedback: "user can select icon for parent category from
    /// disk (png, ico, svg)... saved by the app independently"). Null means "use
    /// IconGlyph instead". Never points at the original file the user picked —
    /// see CategoryIconService.SaveOptimizedIcon, which always writes its own copy
    /// under AppPaths.CategoryIconsDirectory so a later move/delete of the source
    /// file can't break the category's icon.
    /// </summary>
    public string? CustomIconPath { get; set; }
}
