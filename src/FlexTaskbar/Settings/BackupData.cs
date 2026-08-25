using FlexTaskbar.Applications;

namespace FlexTaskbar.Settings;

/// <summary>
/// The export/import payload (spec Section 41): categories, app category/pin
/// assignments, and appearance/keyboard settings. Deliberately excludes the full
/// discovered application list (always regenerable by rescanning) and anything
/// machine-specific-secret (there's nothing credential-like in this app to exclude,
/// but the principle is why this isn't just "dump all four JSON files").
/// </summary>
public sealed class BackupData
{
    public List<ApplicationCategory> Categories { get; set; } = new();
    public List<ApplicationAssignment> AppAssignments { get; set; } = new();
    public AppSettings? Settings { get; set; }
}
