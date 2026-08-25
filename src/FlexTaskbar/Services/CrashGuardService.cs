namespace FlexTaskbar.Services;

/// <summary>
/// Detects repeated crash-on-startup loops (spec Section 47: "after several
/// crashes, disable automatic startup"). Works by a dirty-flag: every startup marks
/// state as "not cleanly exited" and only <see cref="RecordCleanExit"/> (called from
/// <c>App.OnExit</c>) clears it. If a new startup finds the flag still dirty, the
/// previous run never reached a clean exit — i.e. it crashed (or was killed) rather
/// than shutting down normally — and the streak counter increments. A normal
/// graceful close resets the streak to zero.
/// </summary>
public static class CrashGuardService
{
    private const string FileName = "crash_state.json";
    private const int CrashThreshold = 3;

    private sealed class CrashState
    {
        public int ConsecutiveCrashes { get; set; }
        public bool CleanExit { get; set; } = true;
    }

    /// <summary>Call once at startup. Returns true if the crash-loop threshold has
    /// just been reached — the caller should then disable auto-start.</summary>
    public static bool RecordStartupAndCheckCrashLoop()
    {
        var state = ConfigurationService.Load<CrashState>(FileName) ?? new CrashState();

        state.ConsecutiveCrashes = state.CleanExit ? 0 : state.ConsecutiveCrashes + 1;
        state.CleanExit = false; // dirty until RecordCleanExit runs

        ConfigurationService.Save(FileName, state);

        return state.ConsecutiveCrashes >= CrashThreshold;
    }

    /// <summary>Call from a graceful shutdown path (App.OnExit) — marks this run as
    /// having exited cleanly, resetting the crash streak.</summary>
    public static void RecordCleanExit()
    {
        var state = ConfigurationService.Load<CrashState>(FileName) ?? new CrashState();
        state.CleanExit = true;
        state.ConsecutiveCrashes = 0;
        ConfigurationService.Save(FileName, state);
    }
}
