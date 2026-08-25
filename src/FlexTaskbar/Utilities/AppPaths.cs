using System.IO;

namespace FlexTaskbar.Utilities;

/// <summary>
/// Central source of truth for where FlexTaskbar stores its files (spec Section 28):
/// user configuration lives under %APPDATA%, disposable/regenerable caches (icons)
/// live under %LOCALAPPDATA% — never inside the install directory.
/// </summary>
public static class AppPaths
{
    public static string RoamingRoot { get; } = EnsureDirectory(
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "FlexTaskbar"));

    public static string LocalRoot { get; } = EnsureDirectory(
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "FlexTaskbar"));

    public static string IconCacheDirectory { get; } = EnsureDirectory(
        Path.Combine(LocalRoot, "IconCache"));

    /// <summary>App-owned, optimized copies of user-selected category icons (round-2
    /// feedback: custom category icons from disk). Lives under RoamingRoot, not
    /// LocalRoot/IconCacheDirectory, because unlike the app-icon cache these aren't
    /// regenerable from anything else — they're user-provided content.</summary>
    public static string CategoryIconsDirectory { get; } = EnsureDirectory(
        Path.Combine(RoamingRoot, "CategoryIcons"));

    /// <summary>App-owned, optimized copies of user-selected per-app icons — lets
    /// any app added from All Apps (a normal exe or a web app alike) have its icon
    /// changed, the same way category icons already can. Keyed by
    /// <see cref="ApplicationEntry.Id"/>. Roaming, not the regenerable icon cache,
    /// for the same reason as <see cref="CategoryIconsDirectory"/>: user-provided
    /// content, not something re-extractable from the exe/shortcut itself.</summary>
    public static string ApplicationIconsDirectory { get; } = EnsureDirectory(
        Path.Combine(RoamingRoot, "ApplicationIcons"));

    private static string EnsureDirectory(string path)
    {
        Directory.CreateDirectory(path);
        return path;
    }
}
