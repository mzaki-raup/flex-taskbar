using System.IO;
using System.Text.Json;
using FlexTaskbar.Utilities;

namespace FlexTaskbar.Services;

/// <summary>
/// Generic atomic JSON persistence for files under %APPDATA%\FlexTaskbar\ (spec
/// Section 28). Writes go to a temp file and are swapped in with File.Move(overwrite),
/// and the previous version is kept as a ".bak" so a corrupted write can be
/// auto-recovered on the next load.
/// </summary>
public static class ConfigurationService
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
    };

    public static T? Load<T>(string fileName) where T : class
    {
        var path = Path.Combine(AppPaths.RoamingRoot, fileName);
        var value = TryLoad<T>(path);
        if (value is not null)
            return value;

        // Primary file missing/corrupt — fall back to the last known-good backup.
        return TryLoad<T>(path + ".bak");
    }

    public static void Save<T>(string fileName, T data)
    {
        var path = Path.Combine(AppPaths.RoamingRoot, fileName);
        var tempPath = path + ".tmp";
        var backupPath = path + ".bak";

        var json = JsonSerializer.Serialize(data, JsonOptions);
        File.WriteAllText(tempPath, json);

        if (File.Exists(path))
            File.Copy(path, backupPath, overwrite: true);

        File.Move(tempPath, path, overwrite: true);
    }

    private static T? TryLoad<T>(string path) where T : class
    {
        try
        {
            if (!File.Exists(path))
                return null;

            return JsonSerializer.Deserialize<T>(File.ReadAllText(path), JsonOptions);
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
            return null;
        }
    }
}
