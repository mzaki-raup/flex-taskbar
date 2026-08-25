namespace FlexTaskbar.Applications;

/// <summary>
/// How an entry should be launched. Most entries are ordinary executables; ".url"
/// shortcuts (web-app launchers) resolve to a URL instead and are handed to the
/// default browser via ShellExecute rather than run as a process. ShellCommand
/// covers targets that aren't a real file on disk at all — currently just
/// "shell:AppsFolder\{AUMID}" launches (round-5 feedback: pinning an installed
/// PWA/UWP app by its Application User Model ID, e.g. Discord's web app) — handed
/// straight to ShellExecute rather than CreateProcess, since there's no exe path
/// to validate with File.Exists.
/// </summary>
public enum ApplicationLaunchKind
{
    Executable,
    Url,
    ShellCommand,
}

/// <summary>
/// A discovered (or user-configured) launchable item (spec Section 8).
/// <see cref="Id"/> is derived deterministically from the resolved target
/// (<see cref="ExecutablePath"/> for executables, the URL itself for web shortcuts),
/// not from the source shortcut path — this is what makes duplicate detection
/// (Section 36) and preserving pin/category assignments across rescans (Section 2's
/// "safe/recoverable" spirit applied to user customization) work for free: two
/// shortcuts pointing at the same target collapse into the same Id.
/// </summary>
public sealed class ApplicationEntry
{
    public required string Id { get; init; }
    public required string Name { get; set; }
    public required string ExecutablePath { get; set; }
    public ApplicationLaunchKind LaunchKind { get; set; } = ApplicationLaunchKind.Executable;
    public string Arguments { get; set; } = string.Empty;
    public string? WorkingDirectory { get; set; }
    public string? SourceShortcut { get; set; }
    public string? CategoryId { get; set; }
    public bool IsPinned { get; set; }
    public bool IsFavorite { get; set; }
    public bool RunAsAdministrator { get; set; }

    /// <summary>Position in the taskbar's center panel, shared with root
    /// categories' own SortOrder (round feedback: "apps shortcut and category can
    /// be rearranged in order by drag and drop", later extended so the two can be
    /// interleaved with each other: "make also the category can be reorder on the
    /// right with apps shortcut link") — only meaningful while
    /// <see cref="IsPinned"/> is true; see
    /// <see cref="ApplicationManager.GetPinnedApplications"/> and
    /// <see cref="ApplicationManager.ApplyCenterPanelOrder"/>.</summary>
    public int SortOrder { get; set; }

    /// <summary>Position among sibling apps within the same category (round
    /// feedback: "apps inside category can drag and drop") — separate from
    /// <see cref="SortOrder"/> since an app can be pinned *and* categorized at the
    /// same time, and reordering it in one place shouldn't move it in the other.
    /// See <see cref="ApplicationManager.GetApplicationsInCategory"/> and
    /// <see cref="ApplicationManager.SetCategoryPosition"/>.</summary>
    public int CategorySortOrder { get; set; }

    /// <summary>App-owned, optimized copy of a user-selected icon file (see
    /// <see cref="CategoryIconService.SaveOptimizedIcon(string, string, string)"/> and
    /// <see cref="AppPaths.ApplicationIconsDirectory"/>) — lets any app added from
    /// All Apps have its icon changed, whether it's a normal exe or a web app
    /// (<see cref="LaunchKind"/> is irrelevant here; this always takes priority over
    /// native extraction/absence of an icon when set). Null means "use the default
    /// icon" — native shell extraction for executables, none for web apps.</summary>
    public string? CustomIconPath { get; set; }

    /// <summary>
    /// WPF's default ListBoxItem/MenuItem accessible-name derivation falls back to
    /// ToString() when it can't find a simple string to bind to (e.g. our
    /// icon+text DataTemplates). Without this override, every list of apps reports
    /// its raw type name ("FlexTaskbar.Applications.ApplicationEntry") to screen
    /// readers and UI Automation instead of the app name — caught while verifying
    /// the Phase 5 launcher's "All Applications" list against the live app.
    /// </summary>
    public override string ToString() => Name;
}
