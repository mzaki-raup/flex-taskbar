using System.Diagnostics;
using FlexTaskbar.Native;

namespace FlexTaskbar.Services;

/// <summary>
/// System power actions for the Start launcher's Power menu (spec Section 16).
/// Restart/shutdown/sign-out shell out to the standalone <c>shutdown.exe</c> — a
/// real system executable, not <c>cmd.exe</c>/<c>powershell.exe</c> (Section 40) —
/// invoked with an explicit argument list via <c>ArgumentList</c>, never a
/// concatenated command string. This also avoids needing SE_SHUTDOWN privilege
/// elevation, which the raw <c>ExitWindowsEx</c> API requires for restart/shutdown
/// (only Lock does not need it, so Lock uses the native <c>LockWorkStation</c> call
/// directly instead).
/// </summary>
public static class PowerActionService
{
    public static void Lock() => User32.LockWorkStation();

    public static void SignOut() => RunShutdownExe("/l");

    public static void Restart() => RunShutdownExe("/r", "/t", "0");

    public static void Shutdown() => RunShutdownExe("/s", "/t", "0");

    /// <summary>Sleep has no shutdown.exe equivalent; SetSuspendState (powrprof.dll)
    /// would be the native route. Not wired up yet — TODO for a future pass.</summary>
    public static bool SleepSupported => false;

    private static void RunShutdownExe(params string[] args)
    {
        var startInfo = new ProcessStartInfo
        {
            FileName = Environment.ExpandEnvironmentVariables(@"%WINDIR%\System32\shutdown.exe"),
            UseShellExecute = false,
        };

        foreach (var arg in args)
            startInfo.ArgumentList.Add(arg);

        using var process = Process.Start(startInfo);
    }
}
