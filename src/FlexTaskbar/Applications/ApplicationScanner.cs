using System.IO;
using System.Security.Cryptography;
using System.Text;
using FlexTaskbar.Shell;

namespace FlexTaskbar.Applications;

/// <summary>
/// Discovers launchable applications from the Start Menu (machine + user) and Desktop
/// (spec Section 7). Scanning is bounded to those specific roots — it never walks the
/// whole C:\ drive — but recurses fully *within* them, since Start Menu "Programs" is
/// itself organized as a shallow tree of category folders that installers create.
///
/// Duplicate detection (Section 36) falls out of ApplicationEntry.Id being derived
/// from the resolved target rather than the shortcut path: two shortcuts pointing at
/// the same executable collapse into a single entry automatically.
/// </summary>
public sealed class ApplicationScanner
{
    private static readonly string[] SupportedExtensions = { ".lnk", ".url", ".exe" };

    public Task<IReadOnlyList<ApplicationEntry>> ScanAsync(CancellationToken cancellationToken = default)
    {
        return Task.Run(() => Scan(cancellationToken), cancellationToken);
    }

    private static IReadOnlyList<ApplicationEntry> Scan(CancellationToken cancellationToken)
    {
        var results = new Dictionary<string, ApplicationEntry>(StringComparer.OrdinalIgnoreCase);

        foreach (var root in GetScanRoots())
            ScanDirectoryTree(root, results, cancellationToken);

        return results.Values
            .OrderBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
    }

    private static IEnumerable<string> GetScanRoots()
    {
        var candidates = new[]
        {
            CombineSafe(Environment.GetFolderPath(Environment.SpecialFolder.CommonStartMenu), "Programs"),
            CombineSafe(Environment.GetFolderPath(Environment.SpecialFolder.StartMenu), "Programs"),
            Environment.GetFolderPath(Environment.SpecialFolder.CommonDesktopDirectory),
            Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
        };

        foreach (var candidate in candidates)
        {
            if (!string.IsNullOrWhiteSpace(candidate) && Directory.Exists(candidate))
                yield return candidate;
        }
    }

    private static string CombineSafe(string basePath, string child) =>
        string.IsNullOrWhiteSpace(basePath) ? string.Empty : Path.Combine(basePath, child);

    /// <summary>
    /// Manual breadth-first walk instead of Directory.EnumerateFiles(..., AllDirectories):
    /// a single ACL-denied subfolder would otherwise abort the whole lazy enumeration.
    /// Here each directory is tried independently, so one bad folder just gets skipped
    /// (spec Section 34 — must tolerate access-denied without crashing).
    /// </summary>
    private static void ScanDirectoryTree(string root, Dictionary<string, ApplicationEntry> results, CancellationToken cancellationToken)
    {
        var pending = new Queue<string>();
        pending.Enqueue(root);

        while (pending.Count > 0)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var directory = pending.Dequeue();

            string[] subDirectories;
            string[] files;
            try
            {
                subDirectories = Directory.GetDirectories(directory);
                files = Directory.GetFiles(directory);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                continue;
            }

            foreach (var sub in subDirectories)
                pending.Enqueue(sub);

            foreach (var file in files)
            {
                if (!SupportedExtensions.Contains(Path.GetExtension(file), StringComparer.OrdinalIgnoreCase))
                    continue;

                var entry = TryCreateEntry(file);
                if (entry is not null)
                    results[entry.Id] = entry;
            }
        }
    }

    private static ApplicationEntry? TryCreateEntry(string file)
    {
        var extension = Path.GetExtension(file);
        try
        {
            return extension.ToLowerInvariant() switch
            {
                ".exe" => CreateFromExecutable(file),
                ".lnk" => CreateFromShortcut(file),
                ".url" => CreateFromUrlShortcut(file),
                _ => null,
            };
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // A single unreadable shortcut/exe must not abort the whole scan.
            return null;
        }
    }

    private static ApplicationEntry? CreateFromExecutable(string exePath)
    {
        if (!File.Exists(exePath))
            return null;

        return new ApplicationEntry
        {
            Id = ComputeId(exePath),
            Name = Path.GetFileNameWithoutExtension(exePath),
            ExecutablePath = exePath,
            WorkingDirectory = Path.GetDirectoryName(exePath),
            SourceShortcut = exePath,
        };
    }

    private static ApplicationEntry? CreateFromShortcut(string lnkPath)
    {
        var shortcut = ShellIntegration.ResolveShortcut(lnkPath);
        if (shortcut is null)
            return null; // broken/unreadable shortcut — skip silently

        var target = shortcut.TargetPath;
        if (string.IsNullOrWhiteSpace(target) ||
            !string.Equals(Path.GetExtension(target), ".exe", StringComparison.OrdinalIgnoreCase))
        {
            // Not an application shortcut (e.g. points at a folder, document, or a
            // Control Panel/shell namespace item) — out of scope for the app launcher.
            return null;
        }

        if (!File.Exists(target))
            return null; // target was uninstalled; shortcut is stale

        return new ApplicationEntry
        {
            Id = ComputeId(BuildIdSeed(target, shortcut.Arguments)),
            Name = Path.GetFileNameWithoutExtension(lnkPath),
            ExecutablePath = target,
            Arguments = shortcut.Arguments,
            WorkingDirectory = !string.IsNullOrWhiteSpace(shortcut.WorkingDirectory)
                ? shortcut.WorkingDirectory
                : Path.GetDirectoryName(target),
            SourceShortcut = lnkPath,
        };
    }

    /// <summary>
    /// Id was previously computed from the target exe path alone, which is right
    /// for the common "two shortcuts point at the same real app" duplicate-
    /// detection case (Section 36) — but wrong for Chrome/Edge PWA launcher
    /// shortcuts, which all point at the SAME chrome_proxy.exe/msedge_proxy.exe
    /// and are distinguished only by a --app-id=... argument. Without folding
    /// Arguments into the id, every installed PWA collapsed onto one Id and the
    /// scanner's results dictionary (keyed by Id) silently kept only the last one
    /// scanned — every other web app just disappeared, never making it into the
    /// app list or search at all (round-7 feedback: "search for web apps that
    /// added from chrome and edge" turned up nothing because those entries never
    /// existed past the scan). Apps with no arguments (the vast majority) hash
    /// identically to before, so existing persisted Ids for ordinary apps are
    /// unaffected.
    /// </summary>
    private static string BuildIdSeed(string target, string arguments) =>
        string.IsNullOrWhiteSpace(arguments) ? target : $"{target}|{arguments}";

    private static ApplicationEntry? CreateFromUrlShortcut(string urlFilePath)
    {
        string? url = null;
        foreach (var line in File.ReadLines(urlFilePath))
        {
            if (line.StartsWith("URL=", StringComparison.OrdinalIgnoreCase))
            {
                url = line["URL=".Length..].Trim();
                break;
            }
        }

        if (string.IsNullOrWhiteSpace(url))
            return null;

        // Only allow http(s) — never file://, javascript:, or shell-handler schemes
        // from a shortcut we didn't create ourselves (spec Section 40/41: validate
        // imported shortcuts).
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) ||
            (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
        {
            return null;
        }

        return new ApplicationEntry
        {
            Id = ComputeId(uri.ToString()),
            Name = Path.GetFileNameWithoutExtension(urlFilePath),
            ExecutablePath = uri.ToString(),
            LaunchKind = ApplicationLaunchKind.Url,
            SourceShortcut = urlFilePath,
        };
    }

    private static string ComputeId(string target)
    {
        var normalized = target.Trim().ToLowerInvariant();
        var hash = SHA1.HashData(Encoding.UTF8.GetBytes(normalized));
        return Convert.ToHexString(hash)[..16];
    }
}
