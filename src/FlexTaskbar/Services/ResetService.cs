using System.IO;
using System.Linq;
using FlexTaskbar.Applications;
using FlexTaskbar.Settings;
using FlexTaskbar.Utilities;

namespace FlexTaskbar.Services;

/// <summary>
/// Shared reset logic (spec Section 42) — used by both Settings' "Reset
/// Configuration" button and the <c>--reset</c> CLI flag, so there's exactly one
/// place that knows what "reset" means and one place that backs files up first.
/// Deliberately a single full reset rather than the spec's granular per-section
/// resets (appearance/categories/applications/full) — see DEVELOPMENT.md for why
/// that scope cut was made.
/// </summary>
public static class ResetService
{
    private const string ApplicationsFileName = "applications.json";
    private static readonly string[] ResettableFiles = { "categories.json", "settings.json", "recents.json" };

    /// <summary>
    /// Backs up categories/settings/recents (timestamped, kept indefinitely — this
    /// is a deliberate safety action, not routine churn) then resets them to
    /// defaults. <c>applications.json</c>'s own shape/scan data is left untouched
    /// (it's just a regenerable scan cache) — but every app's CategoryId is cleared,
    /// mirroring what <see cref="CategoryManager.DeleteCategory"/> already does for
    /// a single deleted category. Without this, apps that were inside a category
    /// keep pointing at a category id that no longer exists once every category is
    /// wiped — orphaned, showing up neither in any category (there are none left)
    /// nor back in "All Applications" (which only lists apps with a null CategoryId).
    /// </summary>
    public static void PerformFullReset()
    {
        foreach (var fileName in ResettableFiles)
            BackupBeforeReset(fileName);

        ConfigurationService.Save("categories.json", new List<ApplicationCategory>());
        ConfigurationService.Save("settings.json", new AppSettings());
        ConfigurationService.Save("recents.json", new List<string>());

        UncategorizeAllApplications();
    }

    private static void UncategorizeAllApplications()
    {
        var applications = ConfigurationService.Load<List<ApplicationEntry>>(ApplicationsFileName);
        if (applications is not { Count: > 0 })
            return;

        var changed = false;
        foreach (var app in applications.Where(a => a.CategoryId is not null))
        {
            app.CategoryId = null;
            changed = true;
        }

        if (!changed)
            return;

        BackupBeforeReset(ApplicationsFileName);
        ConfigurationService.Save(ApplicationsFileName, applications);
    }

    private static void BackupBeforeReset(string fileName)
    {
        var path = Path.Combine(AppPaths.RoamingRoot, fileName);
        if (!File.Exists(path))
            return;

        var backupPath = Path.Combine(AppPaths.RoamingRoot, $"{fileName}.{DateTime.Now:yyyyMMdd-HHmmss}.resetbackup");
        File.Copy(path, backupPath, overwrite: true);
    }
}
