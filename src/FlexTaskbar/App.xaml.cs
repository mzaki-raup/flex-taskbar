using System.Threading;
using System.Windows;
using System.Windows.Threading;
using FlexTaskbar.Services;
using FlexTaskbar.Taskbar;
using FlexTaskbar.Views;

namespace FlexTaskbar;

public partial class App : Application
{
    // Single global name so a second launch can detect the running instance
    // instead of starting a competing taskbar window. See Section 55.
    private const string SingleInstanceMutexName = "Local\\FlexTaskbar.SingleInstance.9F3B2C7A";

    private Mutex? _singleInstanceMutex;
    private TaskbarWindow? _taskbarWindow;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        // Safety net for spec Section 34/47: an unhandled exception on the UI thread
        // is fatal to the whole process by default — one bad callback in a menu, a
        // dialog, anything, and the entire taskbar (and every window it owns) dies
        // with it. Caught live during Phase 5 testing: a re-entrant Window.Close()
        // bug in LauncherWindow took the whole app down before this handler existed.
        // Marking e.Handled = true keeps the taskbar alive; proper structured logging
        // (Section 33) lands in a later phase, so this writes to stderr for now,
        // which is at least visible when run from a console during development.
        DispatcherUnhandledException += OnDispatcherUnhandledException;

        // Belt-and-suspenders for exceptions that *don't* go through the Dispatcher
        // (a background thread, for instance) — these are still fatal (there's no
        // way to un-terminate the CLR from here), but at least get logged before the
        // process dies instead of vanishing silently.
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
            Console.Error.WriteLine($"[FlexTaskbar] Fatal non-UI-thread exception: {args.ExceptionObject}");

        var options = LaunchOptions.Parse(e.Args);

        _singleInstanceMutex = new Mutex(initiallyOwned: true, SingleInstanceMutexName, out var createdNew);
        if (!createdNew)
        {
            if (options.Restart || options.Reset)
            {
                // Ask the running instance to exit gracefully, then take over its
                // slot rather than just bailing out like a normal second launch
                // would (Section 54).
                var released = InstanceSignalingService.RequestExitAndWait(_singleInstanceMutex, TimeSpan.FromSeconds(5));
                if (!released || !_singleInstanceMutex.WaitOne(TimeSpan.FromSeconds(2)))
                {
                    Console.Error.WriteLine("[FlexTaskbar] The running instance did not exit in time; not starting a second one.");
                    Shutdown();
                    return;
                }
            }
            else
            {
                // TODO(later): locate the existing instance's window and focus it
                // instead of just exiting. Requires a lightweight IPC/window-message
                // handshake with the running instance beyond the restart signal above.
                Shutdown();
                return;
            }
        }

        if (options.Disable)
        {
            StartupService.SetEnabled(false);
            Shutdown();
            return;
        }

        if (options.Reset)
        {
            ResetService.PerformFullReset();
            // Falls through to normal startup below — the freshly-reset taskbar
            // comes up immediately rather than requiring a second manual launch.
        }

        if (options.SafeMode)
        {
            // Section 30: safe mode shows *only* the Settings/Recovery interface,
            // with every taskbar-replacement behavior (AppBar reservation,
            // auto-hide, hotkeys) disabled by simply never constructing
            // TaskbarWindow at all — not a flag threaded through it.
            var recoveryWindow = new RecoveryWindow(taskbarWindow: null, isSafeMode: true);
            recoveryWindow.Closed += (_, _) => Shutdown();
            recoveryWindow.Show();
            return;
        }

        if (CrashGuardService.RecordStartupAndCheckCrashLoop())
        {
            // Section 47: don't retry forever into the same crash on every reboot —
            // disable auto-start, but still launch normally this once so the user
            // isn't locked out of using FlexTaskbar manually.
            StartupService.SetEnabled(false);
            Console.Error.WriteLine("[FlexTaskbar] Disabled auto-start after repeated crashes/unclean exits. Launch manually, or use --safe-mode to investigate. Re-enable in Settings once resolved.");
        }

        _taskbarWindow = new TaskbarWindow();
        _taskbarWindow.Closed += (_, _) => Shutdown();
        _taskbarWindow.Show();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        CrashGuardService.RecordCleanExit();
        _singleInstanceMutex?.ReleaseMutex();
        _singleInstanceMutex?.Dispose();
        base.OnExit(e);
    }

    private static void OnDispatcherUnhandledException(object sender, DispatcherUnhandledExceptionEventArgs e)
    {
        Console.Error.WriteLine($"[FlexTaskbar] Unhandled UI exception (suppressed to keep the taskbar alive): {e.Exception}");
        e.Handled = true;
    }
}
