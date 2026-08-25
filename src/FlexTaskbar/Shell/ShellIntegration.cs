using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using FlexTaskbar.Native;

namespace FlexTaskbar.Shell;

/// <summary>
/// Reads Windows Shell Link (.lnk) files via IShellLinkW/IPersistFile COM interop
/// (spec Section 64) rather than shelling out to WScript.Shell, keeping shortcut
/// parsing inside a typed, injection-free native call.
/// </summary>
public static class ShellIntegration
{
    private const uint STGM_READ = 0x00000000;

    public sealed record ShortcutInfo(
        string TargetPath,
        string Arguments,
        string? WorkingDirectory,
        string? Description,
        string? IconLocation,
        int IconIndex);

    /// <summary>
    /// Resolves a .lnk file. Returns null for anything unreadable/corrupt — callers
    /// are expected to skip the shortcut rather than fail the whole scan (spec
    /// Section 34: "a shortcut is broken" must not crash the app).
    /// </summary>
    public static ShortcutInfo? ResolveShortcut(string lnkPath)
    {
        object? comObject = null;
        try
        {
            comObject = new Shell32.ShellLinkCoClass();
            var link = (Shell32.IShellLinkW)comObject;
            var persistFile = (System.Runtime.InteropServices.ComTypes.IPersistFile)comObject;

            persistFile.Load(lnkPath, (int)STGM_READ);

            var targetBuilder = new StringBuilder(260);
            link.GetPath(targetBuilder, targetBuilder.Capacity, out _, 0);

            var argsBuilder = new StringBuilder(1024);
            link.GetArguments(argsBuilder, argsBuilder.Capacity);

            var workingDirBuilder = new StringBuilder(260);
            link.GetWorkingDirectory(workingDirBuilder, workingDirBuilder.Capacity);

            var descriptionBuilder = new StringBuilder(1024);
            link.GetDescription(descriptionBuilder, descriptionBuilder.Capacity);

            var iconPathBuilder = new StringBuilder(260);
            link.GetIconLocation(iconPathBuilder, iconPathBuilder.Capacity, out var iconIndex);

            return new ShortcutInfo(
                TargetPath: targetBuilder.ToString(),
                Arguments: argsBuilder.ToString(),
                WorkingDirectory: NullIfEmpty(workingDirBuilder.ToString()),
                Description: NullIfEmpty(descriptionBuilder.ToString()),
                IconLocation: NullIfEmpty(iconPathBuilder.ToString()),
                IconIndex: iconIndex);
        }
        catch (Exception ex) when (ex is COMException or UnauthorizedAccessException or IOException or FileNotFoundException)
        {
            return null;
        }
        finally
        {
            if (comObject is not null && Marshal.IsComObject(comObject))
                Marshal.ReleaseComObject(comObject);
        }
    }

    private static string? NullIfEmpty(string value) => string.IsNullOrWhiteSpace(value) ? null : value;
}
