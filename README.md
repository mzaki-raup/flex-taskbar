# FlexTaskbar

A lightweight, native Windows taskbar replacement for people with **many** installed
applications who want them organized into categories and nested submenus instead of a
taskbar full of hundreds of icons.

- **Native C# / .NET 8 / WPF** — no Electron, Chromium, WebView2, WinUI 3, MAUI, Avalonia, or Qt.
- Windows 10 (22H2+) and Windows 11.
- Fully local: no telemetry, no analytics, no network calls.
- Safe by design: never disables Explorer or the real taskbar; screen-space
  reservation (AppBar mode) is opt-in and always recoverable — see
  [RECOVERY.md](docs/RECOVERY.md).

> **Status:** all 9 planned phases complete. See [DEVELOPMENT.md](DEVELOPMENT.md) for
> the full picture — every feature listed there was verified against the actual
> running app (or the built installer), not just compiled successfully. Known gaps
> are documented honestly rather than hidden: no pinned-apps UI yet, no structured
> logging, and a couple of specific items noted in DEVELOPMENT.md's "Known issues"
> section — most notably, an AppBar/Explorer-restart recovery fix from Phase 8 that
> was implemented but not independently re-confirmed by a second live test.

## Installing

```powershell
msiexec /i FlexTaskbar.Installer.msi
```

Per-user install, no administrator privileges required. See
[INSTALLATION.md](docs/INSTALLATION.md) for feature options (desktop shortcut,
start-with-Windows) and how to build the installer yourself.

## Building from source

Requires the .NET 8 SDK or newer (an SDK that includes `net8.0-windows` support, e.g.
.NET 8, 9, or 10) and Windows (WPF only builds/runs on Windows). No Visual Studio
required — the `dotnet` CLI builds everything, including the installer (which
restores the WiX toolset via NuGet automatically).

```bash
dotnet build FlexTaskbar.sln -c Release
```

## Running

```bash
dotnet run --project src/FlexTaskbar/FlexTaskbar.csproj
```

or run the built executable directly:

```
src/FlexTaskbar/bin/Release/net8.0-windows10.0.19041.0/FlexTaskbar.exe
```

### Command-line options

| Flag | Effect |
|---|---|
| *(none)* | Normal startup — docks a taskbar window above the primary monitor's work area. |
| `--safe-mode` | Launches only the Settings/Recovery window — no taskbar, no AppBar reservation, no hotkeys, no auto-hide. |
| `--disable` | Removes the Startup-folder auto-start entry and exits. |
| `--restart` | Gracefully closes any already-running instance, then starts fresh. |
| `--reset` | Backs up (timestamped) and resets categories/settings/recents to defaults, then starts normally. |
| `--settings` | Parsed but not yet wired to anything — planned to open Settings directly without going through the taskbar UI. |

## Testing

```bash
dotnet test FlexTaskbar.sln
```

## Documentation

- [DEVELOPMENT.md](DEVELOPMENT.md) — phase-by-phase history: what's implemented, how it was verified, and every bug found along the way.
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — project layout and design decisions.
- [INSTALLATION.md](docs/INSTALLATION.md) — install/uninstall via the WiX installer.
- [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) — common issues and their real causes.
- [RECOVERY.md](docs/RECOVERY.md) — how to get your normal Windows taskbar back if anything goes wrong.

## Safety

FlexTaskbar is designed to never leave you without a working taskbar:

- It never terminates or disables `explorer.exe` automatically — "Restart Explorer"
  in the Recovery window is user-initiated and confirmation-gated, never automatic.
- It never modifies shell registry configuration.
- Screen-space reservation (AppBar mode) and auto-hide are both **off by default**
  and only take effect once you explicitly enable them in Settings.
- A recovery hotkey (`Ctrl+Alt+Shift+F12`) and `--safe-mode` are always available to
  restore normal behavior — including in a mode that doesn't depend on the taskbar
  itself working. See [RECOVERY.md](docs/RECOVERY.md).

## Privacy

Everything runs locally. No telemetry, no analytics, no cloud services, no network
requests of any kind.

## License

TBD.
