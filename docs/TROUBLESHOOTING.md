# Troubleshooting

## The taskbar window doesn't appear

- Check for another running instance: FlexTaskbar enforces a single-instance mutex
  (`Local\FlexTaskbar.SingleInstance.9F3B2C7A`). A second plain launch exits
  immediately — look for `FlexTaskbar.exe` in Task Manager.
- Confirm you didn't launch with `--safe-mode` (shows only the Recovery window, by
  design — see [RECOVERY.md](RECOVERY.md)) or `--disable` (disables auto-start and
  exits).
- If FlexTaskbar crashed repeatedly on previous launches, crash-loop protection may
  have disabled its own auto-start (`Services/CrashGuardService.cs`, 3+ consecutive
  unclean exits). It still launches fine manually — check for
  `[FlexTaskbar] Disabled auto-start after repeated crashes...` in the console output
  if run from a terminal.

## Build fails with an SDK/TargetFramework error

FlexTaskbar targets `net8.0-windows10.0.19041.0`. You need an SDK that can restore
that TFM's Windows runtime pack — the .NET 8, 9, or 10 SDK all work. Run
`dotnet --list-sdks` to confirm one is installed.

## Build fails with `CS0104` ambiguous reference errors

If you're adding code that touches `System.Windows.Forms` types (the tray icon uses
`NotifyIcon`), don't add a project-wide `using System.Windows.Forms;` — the csproj
deliberately removes the implicit global usings for `System.Windows.Forms` and
`System.Drawing` (see the `<Using Remove>` items and comment in
`FlexTaskbar.csproj`), because combining `UseWPF` and `UseWindowsForms` otherwise
makes `Application`, `Button`, `Point`, `MouseEventArgs`, and several other shared
type names ambiguous project-wide. Add the `using` directive only in the specific
file that needs it (see `Tray/TrayIconService.cs` for the pattern).

## The taskbar crashes when I open a new dialog/window

There's an app-wide safety net (`App.xaml.cs`'s `DispatcherUnhandledException` and
`AppDomain.UnhandledException` handlers) that suppresses UI-thread exceptions to
keep the taskbar alive, logging to stderr instead. If you're seeing a crash anyway:

- Run from a terminal (not double-clicked) so you can see the
  `[FlexTaskbar] Unhandled UI exception...` output — it includes the full stack trace.
- If the exception happens *during* `InitializeComponent()` in a new window (before
  the safety net's normal event-handling path is even active), it can still
  propagate past the handler. Watch especially for XAML property setters (like
  `Slider.Minimum`/`Maximum`) that fire event handlers referencing controls declared
  *later* in the same XAML file, before those controls are connected — this
  specifically bit `SettingsWindow`'s height slider in Phase 7; the fix pattern is a
  null-check at the top of the handler for anything that might fire during
  construction.

## The Win+Space (or other) launcher hotkey doesn't respond

`RegisterHotKey` can legitimately fail if the combination is already claimed —
Win+Space specifically is Windows' own default input-language-switch shortcut on
many systems, and this was confirmed to fail on the actual dev machine this project
was built on. The Start button always opens the same launcher regardless. To use a
different combination: Settings → Keyboard → pick a different preset (Ctrl+Alt+Space,
Ctrl+Shift+Space, or Ctrl+Alt+K).

## Reserving screen space (AppBar mode) didn't survive an Explorer restart

This is a known, documented limitation — see [RECOVERY.md](RECOVERY.md)'s "Known
limitation" section and DEVELOPMENT.md's Phase 8 notes. A 20-second self-healing
timer should re-claim the reservation, but the end-to-end recovery wasn't cleanly
re-confirmed by a second live test. If you hit this: Settings → General → toggle
"Reserve screen space" off and back on to force re-registration immediately, rather
than waiting for the timer.

## The installed app doesn't appear in "Programs and Features" / Add-or-Remove Programs

Known limitation of the current installer — see [INSTALLATION.md](INSTALLATION.md).
The install/uninstall themselves work correctly (verified: `msiexec /i` then
`msiexec /x` cleanly installs and removes all files and shortcuts); only the Control
Panel *display* entry is missing. Use `msiexec /x FlexTaskbar.Installer.msi`, or
re-run the installer (which detects the existing install and offers repair/remove).

## High memory/CPU usage

The Phase 1 empty shell already measured ~113 MB (Release) — essentially WPF/CLR
baseline load cost, not something a specific feature regressed. Measure with a
Release build (`dotnet build -c Release`) before treating a number as a regression;
Debug builds carry JIT/symbol overhead on top. The full app hasn't been separately
re-measured with everything from all 9 phases loaded — see
[ARCHITECTURE.md](ARCHITECTURE.md)'s performance section for the honest current status.

## Where are logs?

Not implemented yet — structured logging under `%LOCALAPPDATA%\FlexTaskbar\Logs\`
(spec Section 33) is still a gap. Currently, unhandled-exception safety nets and a
few diagnostic messages (hotkey registration failures, crash-loop detection) write
to `Console.Error`, which is only visible if you run `FlexTaskbar.exe` from a
terminal rather than double-clicking it or launching via a shortcut.

## I want my normal taskbar back / something looks broken

See [RECOVERY.md](RECOVERY.md) — `Ctrl+Alt+Shift+F12` opens a Recovery window with
"Restore Windows Taskbar" (disables screen-space reservation), "Restart Explorer",
and more, all of which work even if FlexTaskbar's own window is misbehaving.
`FlexTaskbar.exe --safe-mode` gets you the same Recovery window with zero taskbar
code running at all, if things are bad enough that the hotkey doesn't help.
