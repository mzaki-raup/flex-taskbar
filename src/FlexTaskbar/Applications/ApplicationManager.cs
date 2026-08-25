using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using FlexTaskbar.Services;
using FlexTaskbar.Utilities;

namespace FlexTaskbar.Applications;

/// <summary>
/// Owns the live application list (backed by a disk cache so the taskbar has
/// something to show instantly on startup instead of waiting on a fresh scan every
/// time — Section 27) and safe process launching (Section 40/56).
///
/// Launching never goes through a shell string: ProcessStartInfo is built with an
/// explicit FileName/Arguments/WorkingDirectory, never cmd.exe or powershell.exe.
/// </summary>
public sealed class ApplicationManager
{
    private const string CacheFileName = "applications.json";
    private const string RecentsFileName = "recents.json";
    private const int MaxRecents = 10;

    private readonly ApplicationScanner _scanner = new();
    private readonly List<string> _recentAppIds = new();

    public ObservableCollection<ApplicationEntry> Applications { get; } = new();

    /// <summary>Raised whenever an entry's CategoryId (or other persisted field) changes via this manager.</summary>
    public event Action? Changed;

    /// <summary>Loads the cached application list (if any), then triggers a fresh rescan.</summary>
    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        var cached = ConfigurationService.Load<List<ApplicationEntry>>(CacheFileName);
        if (cached is { Count: > 0 })
            ReplaceApplications(cached);

        var recents = ConfigurationService.Load<List<string>>(RecentsFileName);
        if (recents is { Count: > 0 })
            _recentAppIds.AddRange(recents);

        await RescanAsync(cancellationToken).ConfigureAwait(true);
    }

    /// <summary>Most-recently-launched apps, newest first (spec Section 37) — local only, never transmitted anywhere (Section 38).</summary>
    public IEnumerable<ApplicationEntry> GetRecentApplications() =>
        _recentAppIds
            .Select(id => Applications.FirstOrDefault(a => a.Id == id))
            .Where(a => a is not null)!;

    public void ClearRecentApplications()
    {
        _recentAppIds.Clear();
        ConfigurationService.Save(RecentsFileName, _recentAppIds);
        Changed?.Invoke();
    }

    /// <summary>
    /// Re-scans Start Menu/Desktop and merges the result into <see cref="Applications"/>,
    /// preserving user customizations (pin/favorite/category/elevation) for entries
    /// that still exist. Must be called on the UI thread — it mutates the bound
    /// ObservableCollection directly.
    /// </summary>
    public async Task RescanAsync(CancellationToken cancellationToken = default)
    {
        var scanned = await _scanner.ScanAsync(cancellationToken).ConfigureAwait(true);
        MergeWithExisting(scanned);
        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Persists the current in-memory list without rescanning — used after
    /// category/pin/elevation changes so they survive a restart.</summary>
    public void SaveCache() => ConfigurationService.Save(CacheFileName, Applications.ToList());

    /// <summary>
    /// Adds a manually-entered web app (spec: "allow apps from web (Chrome, Edge) to
    /// be on the taskbar") — separate from the automatic .url-shortcut discovery in
    /// ApplicationScanner, this is for pinning an arbitrary URL directly without
    /// needing an existing shortcut file. Only http/https accepted, same validation
    /// as every other URL launch path in the app (Section 40). Re-adding the same
    /// URL updates the existing entry's name rather than creating a duplicate.
    /// </summary>
    public ApplicationEntry AddManualWebApp(string name, string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) ||
            (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
        {
            throw new ArgumentException("Only http:// and https:// URLs are supported.", nameof(url));
        }

        var entry = new ApplicationEntry
        {
            Id = ComputeManualId(uri.ToString()),
            Name = name.Trim(),
            ExecutablePath = uri.ToString(),
            LaunchKind = ApplicationLaunchKind.Url,
            SourceShortcut = null,
        };

        var existing = Applications.FirstOrDefault(a => a.Id == entry.Id);
        if (existing is not null)
            Applications.Remove(existing);

        Applications.Add(entry);
        SaveCache();
        Changed?.Invoke();
        return entry;
    }

    private static string ComputeManualId(string url)
    {
        var hash = SHA1.HashData(Encoding.UTF8.GetBytes(url.Trim().ToLowerInvariant()));
        return Convert.ToHexString(hash)[..16];
    }

    /// <summary>Same reasoning/behavior as ApplicationScanner's private helper of
    /// the same name — kept in sync manually since the two classes don't share a
    /// base. Folds arguments into the id seed only when present, so ordinary apps
    /// (no arguments) hash identically to a plain target-path id.</summary>
    private static string BuildIdSeed(string target, string arguments) =>
        string.IsNullOrWhiteSpace(arguments) ? target : $"{target}|{arguments}";

    /// <summary>True for a registered URI protocol handler like "ms-screenclip:"
    /// or "ms-settings:" — Windows itself resolves these via ShellExecute, same
    /// mechanism as "shell:AppsFolder\..." AUMID launches, just a different
    /// scheme. Deliberately excludes http/https (handled separately by
    /// <see cref="AddManualWebApp"/>/<see cref="ApplicationLaunchKind.Url"/>,
    /// which restrict to just those two schemes) and file (a real path should
    /// fall through to the ordinary executable-path branch instead).</summary>
    private static bool IsProtocolUri(string target) =>
        Uri.TryCreate(target, UriKind.Absolute, out var uri) &&
        uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps &&
        !string.Equals(uri.Scheme, Uri.UriSchemeFile, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Adds a manually-entered app that isn't a plain URL and doesn't have an
    /// existing shortcut file to drag in (round-5 feedback: "for the web apps, if
    /// I want to open the web apps like these example, is it possible?" — a
    /// Chrome PWA launched via chrome_proxy.exe with --app-id, or an installed
    /// PWA/UWP app launched via its shell:AppsFolder AUMID). Two distinct forms:
    /// - <paramref name="executablePath"/> starting with "shell:" (case-insensitive),
    ///   or any other registered URI protocol handler that isn't http/https/file —
    ///   e.g. "ms-screenclip:" for Snipping Tool's screen clip (round feedback:
    ///   "when i want to add ms-screenclip:, it say not found" — it was falling
    ///   through to the File.Exists check below, which a protocol URI obviously
    ///   never passes) — handed straight to ShellExecute; <paramref name="arguments"/>
    ///   is ignored since these targets don't take any (the whole target string,
    ///   AUMID or URI, is what gets launched).
    /// - Anything else — a real executable path plus optional arguments, same
    ///   launch mechanism as every scanner-discovered app (Section 40: no shell
    ///   string, ProcessStartInfo built explicitly).
    /// Re-adding the same target updates the existing entry rather than
    /// duplicating it, same as <see cref="AddManualWebApp"/>.
    /// </summary>
    public ApplicationEntry AddManualShortcut(string name, string executablePath, string arguments)
    {
        executablePath = executablePath.Trim();
        if (string.IsNullOrWhiteSpace(executablePath))
            throw new ArgumentException("Executable path cannot be empty.", nameof(executablePath));

        ApplicationEntry entry;
        if (executablePath.StartsWith("shell:", StringComparison.OrdinalIgnoreCase) || IsProtocolUri(executablePath))
        {
            entry = new ApplicationEntry
            {
                Id = ComputeManualId(executablePath),
                Name = name.Trim(),
                ExecutablePath = executablePath,
                LaunchKind = ApplicationLaunchKind.ShellCommand,
                SourceShortcut = null,
            };
        }
        else
        {
            if (!File.Exists(executablePath))
                throw new ArgumentException($"Executable not found: {executablePath}", nameof(executablePath));

            // Arguments folded into the id — not just the exe path — so two PWAs
            // launched via the same chrome_proxy.exe/msedge_proxy.exe but
            // different --app-id don't collide onto one entry (round-7 feedback;
            // see the matching fix/comment in ApplicationScanner.BuildIdSeed).
            entry = new ApplicationEntry
            {
                Id = ComputeManualId(BuildIdSeed(executablePath, arguments)),
                Name = name.Trim(),
                ExecutablePath = executablePath,
                Arguments = arguments.Trim(),
                WorkingDirectory = Path.GetDirectoryName(executablePath),
                SourceShortcut = null,
            };
        }

        var existing = Applications.FirstOrDefault(a => a.Id == entry.Id);
        if (existing is not null)
            Applications.Remove(existing);

        Applications.Add(entry);
        SaveCache();
        Changed?.Invoke();
        return entry;
    }

    /// <summary>Moves an app into a category (or back to uncategorized when <paramref name="categoryId"/> is null).</summary>
    public void SetCategory(ApplicationEntry entry, string? categoryId)
    {
        entry.CategoryId = categoryId;

        // Lands at the end of that category's tile row rather than colliding with
        // whatever else is already sitting at CategorySortOrder 0.
        if (categoryId is not null)
        {
            var maxOrder = Applications
                .Where(a => a.CategoryId == categoryId && a.Id != entry.Id)
                .Select(a => a.CategorySortOrder)
                .DefaultIfEmpty(-1)
                .Max();
            entry.CategorySortOrder = maxOrder + 1;
        }

        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Clears CategoryId on every app — used after a full configuration
    /// reset (which wipes every category) so apps that were inside a category don't
    /// stay orphaned, pointing at a category id that no longer exists, invisible in
    /// both "All Applications" (which only lists apps with a null CategoryId) and
    /// every now-empty category list. <see cref="ResetService.PerformFullReset"/>
    /// already does the equivalent directly against applications.json for cases
    /// where no ApplicationManager instance is alive yet (the --reset CLI flag);
    /// this is the live-instance counterpart, called by Settings' "Reset
    /// Configuration" so the currently running taskbar reflects it immediately
    /// instead of only after a restart.</summary>
    public void UncategorizeAll()
    {
        foreach (var entry in Applications)
            entry.CategoryId = null;

        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Sets an app's icon to an app-owned optimized copy of a user-selected
    /// image file (mirrors <see cref="CategoryManager.SetCustomIcon"/>) — works for
    /// any app added from All Apps, exe or web app alike, since it's keyed on the
    /// entry's own Id rather than anything derived from <see cref="ApplicationEntry.LaunchKind"/>.</summary>
    public void SetCustomIcon(ApplicationEntry entry, string sourceFilePath)
    {
        entry.CustomIconPath = CategoryIconService.SaveOptimizedIcon(sourceFilePath, AppPaths.ApplicationIconsDirectory, entry.Id);
        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Reverts an app to its default icon (native extraction for
    /// executables, none for web apps).</summary>
    public void ResetIcon(ApplicationEntry entry)
    {
        entry.CustomIconPath = null;
        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Apps the user has explicitly pinned to the taskbar (round-3 feedback:
    /// "user can add apps shortcut on the taskbar, not only opened apps") — shown in
    /// the center panel as shortcuts. Ordered by <see cref="ApplicationEntry.SortOrder"/>
    /// (round feedback: "apps shortcut and category can be rearranged in order by
    /// drag and drop") rather than name, so a manual reorder sticks.</summary>
    public IEnumerable<ApplicationEntry> GetPinnedApplications() =>
        Applications.Where(a => a.IsPinned).OrderBy(a => a.SortOrder).ThenBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase);

    /// <summary>Toggles the "runas" elevation verb for future launches of this
    /// entry (see Launch's UseShellExecute/Verb wiring below). The data field
    /// already existed (RunAsAdministrator, carried across rescans) but nothing
    /// in the UI ever set it — added after a live request for a manual shortcut
    /// (services.msc via mmc.exe) that needs to launch elevated.</summary>
    public void SetRunAsAdministrator(ApplicationEntry entry, bool runAsAdministrator)
    {
        entry.RunAsAdministrator = runAsAdministrator;
        SaveCache();
        Changed?.Invoke();
    }

    public void SetPinned(ApplicationEntry entry, bool pinned)
    {
        entry.IsPinned = pinned;

        // Newly pinned apps land at the end of the shortcut row rather than
        // colliding with everything else at the default SortOrder of 0.
        if (pinned)
        {
            var maxOrder = Applications.Where(a => a.IsPinned && a.Id != entry.Id).Select(a => a.SortOrder).DefaultIfEmpty(-1).Max();
            entry.SortOrder = maxOrder + 1;
        }

        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Applies an externally-computed SortOrder to whichever pinned apps
    /// appear in <paramref name="orderById"/> — used by the taskbar's center panel
    /// (see TaskbarWindow.ReorderCenterPanelItem) so pinned shortcuts can share one
    /// ordering scale with root category buttons (round feedback: "make also the
    /// category can be reorder on the right with apps shortcut link"), letting a
    /// drag interleave an app among categories and vice versa, not just reorder
    /// within its own type.</summary>
    public void ApplyCenterPanelOrder(IReadOnlyDictionary<string, int> orderById)
    {
        foreach (var app in Applications)
        {
            if (orderById.TryGetValue(app.Id, out var order))
                app.SortOrder = order;
        }

        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>Apps directly inside a category, ordered by
    /// <see cref="ApplicationEntry.CategorySortOrder"/> (round feedback: "apps
    /// inside category can drag and drop") rather than name, so a manual reorder
    /// sticks.</summary>
    public IEnumerable<ApplicationEntry> GetApplicationsInCategory(string categoryId) =>
        Applications.Where(a => a.CategoryId == categoryId)
            .OrderBy(a => a.CategorySortOrder)
            .ThenBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase);

    /// <summary>Moves an app into a category and drops it at a specific position
    /// (round feedback: "apps inside category can drag and drop", refined by
    /// "put also indicator inside category app to show where the apps position
    /// will be put" — an index-based drop, like <see cref="ReorderCenterPanelItem"/>
    /// in TaskbarWindow, rather than "onto this exact other app," which needed a
    /// pixel-precise drop and gave no feedback about where the app would land).
    /// Works whether the app is already in this category (a pure reorder) or
    /// moving in from elsewhere (position and category assignment happen
    /// together, in one save/notify).</summary>
    public void SetCategoryPosition(ApplicationEntry dragged, string categoryId, int insertionIndex)
    {
        dragged.CategoryId = categoryId;

        var siblings = GetApplicationsInCategory(categoryId).ToList();
        var draggedIndex = siblings.FindIndex(a => ReferenceEquals(a, dragged));
        if (draggedIndex < 0)
            return;

        siblings.RemoveAt(draggedIndex);
        if (draggedIndex < insertionIndex)
            insertionIndex--; // removing an earlier item shifts everything after it left by one

        siblings.Insert(Math.Clamp(insertionIndex, 0, siblings.Count), dragged);

        for (var i = 0; i < siblings.Count; i++)
            siblings[i].CategorySortOrder = i;

        SaveCache();
        Changed?.Invoke();
    }

    /// <summary>
    /// Pins a shortcut dropped onto the taskbar from outside the app (round-3
    /// feedback: "user can drag and drop shortcut to the taskbar") — a real
    /// Windows .lnk/.exe file dragged from File Explorer or the desktop, as opposed
    /// to the existing internal drag from the "All Applications" popup. Resolves
    /// .lnk targets the same way ApplicationScanner does; if the resolved/dropped
    /// executable already matches a known entry (by Id, i.e. same target path) that
    /// entry is pinned in place rather than creating a duplicate.
    /// </summary>
    public ApplicationEntry? PinShortcutFile(string filePath)
    {
        var extension = Path.GetExtension(filePath).ToLowerInvariant();
        string exePath;
        string name;
        string? arguments = null;
        string? workingDirectory;

        if (extension == ".lnk")
        {
            var shortcut = FlexTaskbar.Shell.ShellIntegration.ResolveShortcut(filePath);
            if (shortcut is null || string.IsNullOrWhiteSpace(shortcut.TargetPath) ||
                !string.Equals(Path.GetExtension(shortcut.TargetPath), ".exe", StringComparison.OrdinalIgnoreCase))
            {
                return null;
            }

            exePath = shortcut.TargetPath;
            name = Path.GetFileNameWithoutExtension(filePath);
            arguments = shortcut.Arguments;
            workingDirectory = !string.IsNullOrWhiteSpace(shortcut.WorkingDirectory)
                ? shortcut.WorkingDirectory
                : Path.GetDirectoryName(exePath);
        }
        else if (extension == ".exe")
        {
            exePath = filePath;
            name = Path.GetFileNameWithoutExtension(filePath);
            workingDirectory = Path.GetDirectoryName(filePath);
        }
        else
        {
            return null; // not something this launcher can run — silently ignored, same spirit as ApplicationScanner
        }

        if (!File.Exists(exePath))
            return null;

        // Same reasoning as AddManualShortcut above: fold arguments into the id
        // so distinct PWAs sharing one launcher exe don't collide.
        var id = ComputeManualId(BuildIdSeed(exePath, arguments ?? string.Empty));
        var existing = Applications.FirstOrDefault(a => a.Id == id);
        if (existing is not null)
        {
            existing.IsPinned = true;
            SaveCache();
            Changed?.Invoke();
            return existing;
        }

        var entry = new ApplicationEntry
        {
            Id = id,
            Name = name,
            ExecutablePath = exePath,
            Arguments = arguments ?? string.Empty,
            WorkingDirectory = workingDirectory,
            SourceShortcut = null, // manually pinned, not scanner-discovered — see MergeWithExisting's "manualEntries" handling
            IsPinned = true,
        };

        Applications.Add(entry);
        SaveCache();
        Changed?.Invoke();
        return entry;
    }

    /// <summary>User-customization subset for export (spec Section 41) — only apps
    /// that actually have some customization, not the full (regenerable) list.</summary>
    public IEnumerable<ApplicationAssignment> GetAssignments() =>
        Applications
            .Where(a => a.CategoryId is not null || a.IsPinned || a.IsFavorite)
            .Select(a => new ApplicationAssignment
            {
                ApplicationId = a.Id,
                CategoryId = a.CategoryId,
                IsPinned = a.IsPinned,
                IsFavorite = a.IsFavorite,
                SortOrder = a.SortOrder,
                CategorySortOrder = a.CategorySortOrder,
            });

    /// <summary>Applies imported assignments to whichever currently-scanned apps
    /// match by Id — apps not present on this machine are silently skipped rather
    /// than erroring, since imports may cross machines with different installs.</summary>
    public void ApplyAssignments(IEnumerable<ApplicationAssignment> assignments)
    {
        var byId = Applications.ToDictionary(a => a.Id);
        foreach (var assignment in assignments)
        {
            if (!byId.TryGetValue(assignment.ApplicationId, out var app))
                continue;

            app.CategoryId = assignment.CategoryId;
            app.IsPinned = assignment.IsPinned;
            app.IsFavorite = assignment.IsFavorite;
            app.SortOrder = assignment.SortOrder;
            app.CategorySortOrder = assignment.CategorySortOrder;
        }

        SaveCache();
        Changed?.Invoke();
    }

    private void MergeWithExisting(IReadOnlyList<ApplicationEntry> scanned)
    {
        var existingById = Applications.ToDictionary(a => a.Id);

        foreach (var entry in scanned)
        {
            if (!existingById.TryGetValue(entry.Id, out var existing))
                continue;

            entry.IsPinned = existing.IsPinned;
            entry.IsFavorite = existing.IsFavorite;
            entry.CategoryId = existing.CategoryId;
            entry.RunAsAdministrator = existing.RunAsAdministrator;
            entry.CustomIconPath = existing.CustomIconPath;
            entry.SortOrder = existing.SortOrder;
            entry.CategorySortOrder = existing.CategorySortOrder;
        }

        // Manually-added entries (web apps — see AddManualWebApp) have no
        // SourceShortcut, since the scanner never produced them; without this they'd
        // silently disappear on the next rescan.
        var manualEntries = Applications.Where(a => a.SourceShortcut is null).ToList();

        ReplaceApplications(scanned.Concat(manualEntries));
    }

    private void ReplaceApplications(IEnumerable<ApplicationEntry> entries)
    {
        Applications.Clear();
        foreach (var entry in entries.OrderBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase))
            Applications.Add(entry);
    }

    /// <summary>
    /// Launches an application entry. Returns false (never throws) on any failure —
    /// a missing executable, a broken shortcut, or a denied elevation prompt must
    /// never crash the taskbar (Section 34).
    /// </summary>
    public bool Launch(ApplicationEntry entry)
    {
        try
        {
            using var process = Process.Start(BuildStartInfo(entry));
            if (process is null)
                return false;

            RecordRecent(entry.Id);
            return true;
        }
        catch (Exception ex) when (ex is Win32Exception or FileNotFoundException or InvalidOperationException)
        {
            return false;
        }
    }

    private void RecordRecent(string appId)
    {
        _recentAppIds.Remove(appId);
        _recentAppIds.Insert(0, appId);
        if (_recentAppIds.Count > MaxRecents)
            _recentAppIds.RemoveRange(MaxRecents, _recentAppIds.Count - MaxRecents);

        ConfigurationService.Save(RecentsFileName, _recentAppIds);
        Changed?.Invoke();
    }

    private static ProcessStartInfo BuildStartInfo(ApplicationEntry entry)
    {
        if (entry.LaunchKind == ApplicationLaunchKind.Url)
            return BuildUrlStartInfo(entry.ExecutablePath);

        if (entry.LaunchKind == ApplicationLaunchKind.ShellCommand)
            return new ProcessStartInfo { FileName = entry.ExecutablePath, UseShellExecute = true };

        if (!File.Exists(entry.ExecutablePath))
            throw new FileNotFoundException("Executable no longer exists.", entry.ExecutablePath);

        var workingDirectory = entry.WorkingDirectory is { Length: > 0 } dir && Directory.Exists(dir)
            ? dir
            : Path.GetDirectoryName(entry.ExecutablePath) ?? string.Empty;

        var startInfo = new ProcessStartInfo
        {
            FileName = entry.ExecutablePath,
            WorkingDirectory = workingDirectory,
            UseShellExecute = entry.RunAsAdministrator, // required for the "runas" verb
        };

        if (!string.IsNullOrEmpty(entry.Arguments))
            startInfo.Arguments = entry.Arguments;

        if (entry.RunAsAdministrator)
            startInfo.Verb = "runas";

        return startInfo;
    }

    private static ProcessStartInfo BuildUrlStartInfo(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) ||
            (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
        {
            throw new InvalidOperationException("Refusing to launch a URL shortcut with an unexpected scheme.");
        }

        return new ProcessStartInfo
        {
            FileName = uri.ToString(),
            UseShellExecute = true,
        };
    }
}
