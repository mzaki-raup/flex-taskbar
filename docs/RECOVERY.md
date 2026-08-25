# Recovery

FlexTaskbar is designed so you are never stuck without a usable taskbar. As of
Phase 8, the mechanisms below are real and live-verified — not aspirational.

## FlexTaskbar's own recovery mechanism

- **Recovery hotkey**: `Ctrl+Alt+Shift+F12` (from a normally running FlexTaskbar)
  opens a "FlexTaskbar Recovery" window with:
  - **Restore Windows Taskbar** — disables screen-space reservation (AppBar mode)
    and auto-hide, both saved to `settings.json` and applied immediately if
    FlexTaskbar is running.
  - **Restart Explorer** — restarts `explorer.exe` (asks for confirmation first;
    your desktop and real taskbar will flicker/reload for a few seconds).
  - **Disable FlexTaskbar Auto-Start** — removes the Startup-folder shortcut.
  - **Open Settings** — only enabled when a live taskbar instance exists (see
    Safe Mode below).
  - **Restart in Safe Mode** — relaunches FlexTaskbar with `--safe-mode` and exits
    the current instance.
  - This window is deliberately built to work **without** a live `TaskbarWindow` —
    every action operates on the saved configuration files or the OS directly, and
    only *additionally* updates a live instance if one exists. This is what makes
    it usable even if the main taskbar code path is broken.

- **`FlexTaskbar.exe --safe-mode`** launches *only* the Recovery window described
  above — no taskbar, no AppBar reservation, no hotkeys, no auto-hide. Use this if
  FlexTaskbar won't start normally, or you want to fix settings without any
  taskbar-replacement behavior active. Verified: launching with this flag creates
  no `TaskbarWindow` at all, and every Recovery action still works from here.

- **`FlexTaskbar.exe --disable`** turns off auto-start without needing the app to
  run successfully first — it just removes the Startup-folder shortcut and exits.

- **`FlexTaskbar.exe --restart`** gracefully closes any already-running instance
  and starts fresh — useful after changing settings that need a clean restart, or
  if the running instance seems stuck.

- **`FlexTaskbar.exe --reset`** backs up (timestamped) and resets
  `categories.json`/`settings.json`/`recents.json` to defaults, then starts
  normally. Your discovered applications are unaffected — they're re-scanned
  regardless of this flag.

- **Crash-loop protection**: if FlexTaskbar exits uncleanly three times in a row
  (crashes, or is killed, without ever reaching a graceful shutdown), it
  automatically disables its own Startup-folder auto-start on the next launch —
  so a bug that crashes on every boot doesn't keep re-triggering itself forever.
  It still launches that one time rather than refusing to start. A single clean
  exit resets the count to zero.

## What's still recoverable manually

- **If FlexTaskbar's window itself is unresponsive** and the recovery hotkey
  doesn't respond (it's a global hotkey, so it should work regardless, but if it's
  also claimed by another app): open Task Manager (`Ctrl+Shift+Esc`), end
  `FlexTaskbar.exe`, then relaunch with `FlexTaskbar.exe --safe-mode` from a
  terminal to get to the Recovery window without the taskbar running at all.

- **Reserved screen space stuck**: if FlexTaskbar was killed forcibly (not via a
  graceful close) while AppBar reservation was active, the reserved work area can
  in principle outlive the process until Explorer notices and cleans up — in
  practice Explorer reliably handles this, and this hasn't been observed to persist
  across FlexTaskbar's own testing. If it ever does: `--safe-mode` → "Restore
  Windows Taskbar" fixes the *setting* so it won't reserve again, and restarting
  Explorer (also available from Recovery) resets the shell's own AppBar bookkeeping.

## Hard fallback (any Windows install, no FlexTaskbar involvement)

If something goes wrong and you don't have access to any of the above:

1. `Ctrl+Shift+Esc` → Task Manager → end `FlexTaskbar.exe`.
2. If the real Windows taskbar is somehow not visible (should not happen — FlexTaskbar
   never disables it): Task Manager → File → Run new task → `explorer.exe`.

## What FlexTaskbar will *never* do automatically

- Kill or disable `explorer.exe` without you explicitly choosing "Restart Explorer"
  from the Recovery window (and confirming the prompt).
- Modify shell registry configuration (`HKCU\...\Explorer\...` shell-replacement keys).
- Make itself impossible to close or uninstall.
- Reserve screen space (AppBar mode) or auto-hide by default — both are off unless
  you explicitly turned them on via Settings or the recovery menu.

## Known limitation (honestly documented, not swept under the rug)

Explorer-restart tolerance for AppBar reservation has two layers: a fast path that
reacts to Explorer's own "restarted" broadcast message, and a 20-second polling
fallback that re-asserts the reservation regardless of whether that message arrived.
The fast path was found, during live testing, to not reliably fire in at least one
test environment; the polling fallback exists specifically because of that finding.
The fallback's end-to-end recovery (specifically: does it actually restore a
reservation Explorer has fully forgotten about, not just reposition an existing one)
has not yet been independently re-confirmed by a clean live test — see
[DEVELOPMENT.md](../DEVELOPMENT.md)'s Phase 8 notes for the full story. If you rely
on AppBar reservation mode, restarting Explorer while it's active is worth testing
on your own machine before depending on it.
