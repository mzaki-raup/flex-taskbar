using System.Threading;

namespace FlexTaskbar.Services;

/// <summary>
/// Lets a new <c>FlexTaskbar.exe --restart</c>/<c>--reset</c> invocation ask an
/// already-running instance to shut itself down gracefully, instead of the new
/// process just bailing out at the single-instance mutex check like a normal second
/// launch would (spec Section 54). Built on a named <see cref="EventWaitHandle"/> —
/// no window messages needed since this has to work before the new process even
/// has a window.
/// </summary>
public static class InstanceSignalingService
{
    private const string EventName = "Local\\FlexTaskbar.RestartRequested.9F3B2C7A";

    /// <summary>The running instance calls this once at startup; the returned handle
    /// fires <see cref="EventWaitHandle"/> when another process wants it to exit.</summary>
    public static EventWaitHandle CreateListener() =>
        new(initialState: false, EventResetMode.AutoReset, EventName);

    /// <summary>
    /// Signals any running instance to exit, then waits up to <paramref name="timeout"/>
    /// for it to actually release the single-instance mutex. Returns true once the
    /// mutex is free (whether because an instance exited, or none was running to
    /// begin with — opening the event never throws if no listener exists yet, since
    /// EventWaitHandle creates the OS object on first use regardless of order).
    /// </summary>
    public static bool RequestExitAndWait(Mutex singleInstanceMutex, TimeSpan timeout)
    {
        using var signalEvent = new EventWaitHandle(false, EventResetMode.AutoReset, EventName);
        signalEvent.Set();

        try
        {
            var acquired = singleInstanceMutex.WaitOne(timeout);
            if (acquired)
                singleInstanceMutex.ReleaseMutex();
            return acquired;
        }
        catch (AbandonedMutexException)
        {
            // The other process died holding it — that counts as "it exited."
            return true;
        }
    }
}
