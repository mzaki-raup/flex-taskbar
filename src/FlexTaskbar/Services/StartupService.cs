using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using FlexTaskbar.Native;

namespace FlexTaskbar.Services;

/// <summary>
/// Per-user auto-start via a shortcut in the Startup folder (spec Section 31) —
/// deliberately not a registry Run key, since a Startup-folder shortcut needs no
/// admin privilege, is trivially visible/removable by the user (right-click →
/// Open file location), and is what Task Manager's own "Startup" tab already
/// understands natively.
/// </summary>
public static class StartupService
{
    private const string ShortcutName = "FlexTaskbar.lnk";

    private static string ShortcutPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.Startup), ShortcutName);

    public static bool IsEnabled => File.Exists(ShortcutPath);

    public static void SetEnabled(bool enabled)
    {
        if (enabled)
            CreateShortcut();
        else
            RemoveShortcutIfPresent();
    }

    private static void CreateShortcut()
    {
        var exePath = Process.GetCurrentProcess().MainModule?.FileName;
        if (string.IsNullOrEmpty(exePath))
            return;

        object? comObject = null;
        try
        {
            comObject = new Shell32.ShellLinkCoClass();
            var link = (Shell32.IShellLinkW)comObject;
            link.SetPath(exePath);
            link.SetWorkingDirectory(Path.GetDirectoryName(exePath) ?? string.Empty);
            link.SetDescription("FlexTaskbar");

            var persistFile = (System.Runtime.InteropServices.ComTypes.IPersistFile)comObject;
            persistFile.Save(ShortcutPath, false);
        }
        finally
        {
            if (comObject is not null && Marshal.IsComObject(comObject))
                Marshal.ReleaseComObject(comObject);
        }
    }

    private static void RemoveShortcutIfPresent()
    {
        if (File.Exists(ShortcutPath))
            File.Delete(ShortcutPath);
    }
}
