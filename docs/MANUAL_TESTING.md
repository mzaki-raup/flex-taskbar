# Manual Test Plan

Automated coverage (`dotnet test`) is deliberately narrow — it covers pure logic
(search matching, hotkey preset resolution, backup JSON round-tripping, config
persistence) that doesn't need a live window or real Win32 state. Everything that
*does* need those was verified by hand against the actual running app during
development (see [DEVELOPMENT.md](../DEVELOPMENT.md)'s per-phase "verified live"
notes for exactly what was tested, how, and what each test found).

This document is the matrix a release should be checked against — what's already
been exercised (on one specific dev machine/configuration) vs. what still needs a
dedicated pass on different hardware/configurations before shipping.

## Already exercised during development (one Windows 11, single-4K-monitor, 100%-scale machine)

| Area | Status | Notes |
|---|---|---|
| Application discovery (Start Menu + Desktop) | ✅ Verified | Found 200 real installed apps |
| Icon extraction + caching | ✅ Verified | Icons render correctly in all lists/menus |
| Application launching | ✅ Verified | Real process launch confirmed |
| Category CRUD (create/rename/delete/move) | ✅ Verified | Including cascade-safe delete |
| Category flyout menus (nested) | ✅ Verified | Native `ContextMenu`/`MenuItem` tree |
| Drag app → category | ⚠️ Code-reviewed only | Synthetic input can't drive real drag gestures |
| Running window detection/grouping | ✅ Verified | Real desktop windows, including multi-window grouping |
| Window activate/minimize/maximize/close | ✅ Verified | Confirmed via `GetForegroundWindow()` |
| Start launcher + search | ✅ Verified | Substring + category-name matching |
| Global hotkey (Win+Space + presets) | ✅ Verified | Including the documented Win+Space OS conflict |
| Multi-monitor enumeration | ✅ Verified | Against real display hardware |
| AppBar screen-space reservation | ✅ Verified | Real work-area shrink/restore, confirmed independently |
| Auto-hide | ⚠️ Code-reviewed only | Physical mouse-hover simulation unreliable in dev environment |
| System tray icon | ✅ Verified | No errors on startup; menu items present |
| Settings UI (all tabs) | ✅ Verified | Every control drives real, confirmed state |
| Startup folder toggle | ✅ Verified | Shortcut file creation/removal confirmed on disk |
| Import/export (data model) | ✅ Verified | JSON round-trip unit-tested |
| Import/export (file dialogs) | ⚠️ Not exercised | Native `SaveFileDialog`/`OpenFileDialog` UI not automatable here |
| Reset configuration | ✅ Verified | Backup-then-reset confirmed via CLI (`--reset`) and Settings |
| `--safe-mode` | ✅ Verified | Confirmed zero taskbar windows constructed |
| Recovery hotkey (`Ctrl+Alt+Shift+F12`) | ✅ Verified | Real key injection, both standalone and with live taskbar |
| `--restart` / `--reset` cross-process signaling | ✅ Verified | PID handoff confirmed |
| Crash-loop protection | ✅ Verified | Real forced-kill crashes, not mocked |
| Explorer-restart tolerance (AppBar) | ⚠️ Partial | Found and fixed a real bug; the fix itself not re-confirmed by a clean second test — see DEVELOPMENT.md Phase 8 |
| Installer install/uninstall | ✅ Verified | Real `msiexec /i` and `/x`, files/shortcuts confirmed on disk |
| Installer ARP (Programs & Features) listing | ❌ Known gap | Product registers internally but doesn't appear in the Control Panel list |

## Needs a dedicated pass before release (not yet tested at all)

- **Windows 10** — everything above was tested on Windows 11 only.
- **Multiple/mixed-DPI monitors** — enumeration works, but nothing has been tested
  with more than one physical display, or with displays at different scale factors
  from each other.
- **DPI scaling other than 100%** — the Phase 6 DPI conversion bug was fixed and
  reasoned through carefully, but the actual dev machine runs at 100% scaling, so
  the fix itself hasn't been exercised live at 125%/150%/175%/200%.
- **Dark/light mode** — FlexTaskbar's own theme is a fixed dark palette (Section 26's
  Light/System options aren't implemented); "dark/light mode" testing here would
  mean confirming FlexTaskbar's fixed dark UI doesn't look broken against either
  Windows theme, which hasn't been specifically checked.
- **Monitor disconnect while running** — not tested. `MonitorService.GetPrimary()`
  is called fresh each time it's needed rather than cached, which should degrade
  reasonably, but this is inference, not a verified behavior.
- **Application crashes** (a launched app crashing, not FlexTaskbar) — not
  specifically tested; `ApplicationManager.Launch` doesn't track process lifetime
  after a successful `Process.Start`, so a launched app crashing shouldn't affect
  FlexTaskbar at all, but this hasn't been deliberately triggered and observed.
- **A genuinely broken/misbehaving FlexTaskbar** invoking Recovery — Recovery's
  design (works without a live `TaskbarWindow`) was verified in the *normal*
  case (Safe Mode with no taskbar constructed), but not against an actual hung or
  crashed-but-still-running `TaskbarWindow` process.

## How to run the "already exercised" checks yourself

Most of the ✅ items above can be re-checked manually:

1. Build: `dotnet build FlexTaskbar.sln -c Release`
2. Run: `src/FlexTaskbar/bin/x64/Release/net8.0-windows10.0.19041.0/FlexTaskbar.exe`
3. Click Start → confirm search/launcher opens; type a few characters, confirm
   filtering.
4. Right-click the taskbar background → Settings → walk each tab, toggling controls
   and confirming the taskbar visibly responds.
5. `Ctrl+Alt+Shift+F12` → confirm the Recovery window opens.
6. Close normally, then `FlexTaskbar.exe --safe-mode` → confirm only Recovery opens.
7. `FlexTaskbar.exe --reset` → confirm categories/settings reset (back up anything
   you care about first — this is real, not a dry run).
