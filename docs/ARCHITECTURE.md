# Architecture

## Solution layout

```
FlexTaskbar.sln
src/
  FlexTaskbar/            WPF application (net8.0-windows10.0.19041.0)
    Taskbar/               Main taskbar window, AppBar layout, position/height
    Applications/           Discovery, icons, categories, search, launching
    Windows/                Running-window detection & control (SetWinEventHook, EnumWindows)
    Menus/                  Category flyout menus, category management, text-input dialog
    Tray/                   System tray icon (NotifyIcon-based)
    Shell/                  .lnk (shortcut) reading via IShellLinkW
    Settings/               Settings window, persisted preferences, hotkey presets, backup/export model
    Services/               Cross-cutting services — see below
    Native/                 Centralized P/Invoke wrappers
    Views/                  Launcher (search/Start menu) and Recovery windows
    Utilities/              AppPaths, the ApplicationEntry→icon XAML converter
    Resources/              Theme (XAML resource dictionary), app icon
  FlexTaskbar.Tests/       xUnit tests — pure-logic coverage; anything requiring
                             real Win32 state or a live window was verified by
                             hand against the running app instead (see
                             DEVELOPMENT.md's per-phase "verified live" notes)
installer/                 WiX v6 per-user MSI installer
docs/                      This file and friends
```

## Design principles

1. **No shell string execution.** Applications are launched via `ProcessStartInfo`
   with explicit `FileName`/`Arguments`/`WorkingDirectory`, never by building a command
   string and handing it to `cmd.exe` or `powershell.exe`. Power actions
   (`Services/PowerActionService.cs`) shell out to the standalone `shutdown.exe` the
   same way, with an explicit `ArgumentList`, never a concatenated string.
2. **Centralized P/Invoke.** All native Windows API declarations live under `Native/`
   as typed wrappers (`User32.cs`, `Shell32.cs`, `AppBar.cs`, `DwmApi.cs`,
   `NativeTypes.cs`), not scattered `[DllImport]` calls throughout feature code.
3. **Event-driven over polling.** Window state changes use `SetWinEventHook`
   (`Windows/WindowWatcher.cs`), not a tight `EnumWindows` polling loop. The one
   deliberate exception is `TaskbarWindow`'s AppBar-reservation health timer, which
   polls every 20s specifically *because* the event-driven `WM_TASKBARCREATED` path
   was found unreliable in testing — see DEVELOPMENT.md's Phase 8 notes.
4. **Everything local.** No network calls anywhere in the codebase. If you're adding
   an `HttpClient` call, you're in the wrong project.
5. **Never break the real taskbar by default.** Nothing disables, kills, or
   reconfigures `explorer.exe` automatically — "Restart Explorer" in the Recovery
   window is the one exception, and it's a user-initiated, confirmation-gated action,
   never automatic. AppBar screen-space reservation and auto-hide are both off by
   default and only apply once a user explicitly enables them. See
   [RECOVERY.md](RECOVERY.md).
6. **Incremental, honest UI.** A control only appears once its behavior is
   implemented. Where a feature genuinely isn't built yet (Settings' disabled state
   for Safe Mode, `--settings` unwired), it's visibly disabled with an explanation,
   not a dead-looking live control.
7. **Physical pixels vs. DIPs.** Win32 coordinate APIs (`GetMonitorInfo`,
   `SHAppBarMessage`) work in physical pixels; WPF's `Window.Left/Top/Width/Height`
   are in DIPs. `TaskbarLayoutManager` converts explicitly via
   `VisualTreeHelper.GetDpi`. This bit a real bug in Phase 6 — see DEVELOPMENT.md.

## Application lifecycle

`App.xaml.cs`:

1. Installs `DispatcherUnhandledException` (UI thread) and
   `AppDomain.CurrentDomain.UnhandledException` (everything else) handlers first,
   before anything that could throw — a single bad callback anywhere in the app must
   not take the whole taskbar down (this was tested live and caught a real bug in
   Phase 5).
2. Parses CLI options (`LaunchOptions`) and acquires the single-instance mutex.
   `--restart`/`--reset` signal a running instance to exit gracefully
   (`Services/InstanceSignalingService`) rather than just bouncing off the mutex.
3. Branches on `--disable` (remove Startup shortcut, exit), `--reset` (backup +
   reset config, then fall through to normal startup), and `--safe-mode` (show only
   `Views/RecoveryWindow`, never construct `TaskbarWindow` at all).
4. Checks `Services/CrashGuardService` for a crash-loop (3+ consecutive unclean
   exits) and disables auto-start if found, then constructs and shows
   `TaskbarWindow` for the normal path.

`TaskbarWindow` owns essentially all live state for the normal-mode taskbar:
`ApplicationManager`, `CategoryManager`, `SettingsManager`, the running-windows
dictionary, and every service (hotkeys, tray icon, layout manager, window watcher).
It's the one class `SettingsWindow` and `RecoveryWindow` reach into (via a handful
of `internal` accessors) to drive real state rather than duplicating it.

## Configuration

Under `%APPDATA%\FlexTaskbar\`, all via `Services/ConfigurationService` (atomic
write-then-replace, `.bak` fallback on load failure):

- `applications.json` — discovered/cached application entries (regenerable by rescan)
- `categories.json` — category tree
- `recents.json` — recently-launched app IDs (local only, never transmitted)
- `settings.json` — position/height/auto-hide/AppBar/hotkey/startup preferences
- `crash_state.json` — crash-loop tracking (not user-facing config, but same
  persistence mechanism)

Icons are cached separately under `%LOCALAPPDATA%\FlexTaskbar\IconCache\` (disposable
cache, not configuration — see `Utilities/AppPaths.cs`, which is the single source
of truth for this config-vs-cache split).

Export/import (`Settings/BackupData.cs`) bundles categories, app category/pin/
favorite assignments, and settings into one portable JSON file — deliberately not
the full application list, which is always regenerable.

## Recovery architecture

`Views/RecoveryWindow` takes a **nullable** `TaskbarWindow?` and is designed to work
either way: every action operates on persisted config or the OS directly first, and
only *additionally* touches a live taskbar instance if one was passed in. This is
what makes `--safe-mode` (no taskbar constructed at all) and the
`Ctrl+Alt+Shift+F12` hotkey (from a live, possibly-misbehaving taskbar) both work
through the same window with the same code, rather than needing two separate
recovery implementations.

## Performance budget

Target: near-zero idle CPU, <100 MB RAM, fast startup. Measured Phase 1 baseline
(empty shell) was already ~113 MB in Release — essentially all WPF/CLR load cost,
not FlexTaskbar-specific — and the full app across all 9 phases hasn't been
separately re-measured. This is a genuinely open item, not a solved one; see
DEVELOPMENT.md's "Known issues" for the current honest status.
