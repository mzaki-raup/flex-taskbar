# FlexTaskbar — Development Status

## Current phase

**All 9 phases complete.** FlexTaskbar has a working WiX installer, discovers and
launches real applications, organizes them into nested categories, tracks and
controls real running windows, has a Start-menu-style launcher with search and a
configurable global hotkey, supports multi-monitor-aware AppBar screen-space
reservation, has a full Settings UI + system tray + Startup integration, and a
recovery/safe-mode path that works even without a live taskbar. See "Known issues"
below for what's honestly still missing or unverified — this is a complete initial
implementation, not a finished, audited, ready-for-general-release product.

## Post-Phase-9 polish pass (real user feedback after actual use)

After all 9 phases, the app was actually used day-to-day, which surfaced a batch of
real usability issues no amount of scripted testing had caught. All fixed and
live-verified (via screenshots of the real running app, not just UI Automation trees
this time — see below for why that distinction mattered):

- **Taskbar not staying on top of other apps.** `Window.Topmost="True"` only asserts
  the topmost z-order band once; other apps setting themselves topmost afterward
  could still end up drawn above it. Fixed with a 3-second `SetWindowPos(HWND_TOPMOST,
  ...)` reassertion timer (`TaskbarWindow.ReassertTopmost`) — the same technique real
  taskbar-replacement tools use. Verified: a maximized browser game window no longer
  covers the taskbar.
- **No visual gap between Start/All/+.** Added `Margin="0,0,4,0"` to the shared
  `TaskbarButtonStyle` so it applies consistently everywhere that style is used, not
  just those three buttons.
- **Taskbar too short.** Default height changed from 40px to 48px (`AppSettings` and
  `TaskbarWindow`'s field default both updated) — matches the real Windows 11
  taskbar's default height at 100% scaling.
- **Running-window buttons redesigned**: icon-only now (no text label), per explicit
  feedback that labels made the center panel cramped. Hovering shows a popup listing
  that app's window(s) upward (`AttachHoverWindowList`/`ShowHoverWindowList`, with a
  shared 250ms close-delay timer so the mouse can travel from the button into the
  popup without it closing prematurely); right-clicking shows Restore/Minimize/
  Maximize/Close (nested per-window for grouped buttons).
- **System tray indicators were entirely missing** (Section 14 asks for Network/
  Volume/Battery, only the clock existed). Added `Services/SystemStatusService.cs`:
  real network status (`NetworkInterface.GetIsNetworkAvailable()`, event-driven via
  `NetworkChange.NetworkAvailabilityChanged`) and real battery status
  (`SystemInformation.PowerStatus` — battery indicator only shows at all on machines
  that actually have one, never a fake placeholder on a desktop). Volume
  deliberately does *not* show a live level — reading/setting system volume needs
  the Core Audio COM API, a meaningfully bigger interop surface than the rest of
  this file — so the volume button just opens Windows' own volume mixer, honestly
  scoped rather than faking a reading. All three (plus the clock) open the
  corresponding `ms-settings:` page on click.
- **Category icons weren't customizable** (Section 49). Added a curated 19-glyph
  picker to `CategoryManagementWindow` (`CategoryManager.SetIcon`) — real image-file
  picking would be a bigger scope increase than this feedback asked for; the picker
  is fully functional and verified via screenshot + `categories.json` inspection.
- **No way to pin a web app without an existing shortcut.** Added
  `ApplicationManager.AddManualWebApp` — a "+ Web App" button in the "All
  Applications" popup prompts for a name and URL (http/https validated, same as
  every other URL-launch path). This required a real fix to `MergeWithExisting`:
  manually-added entries have no `SourceShortcut` (the scanner never produced them),
  and the existing merge logic would have silently deleted them on the very next
  rescan. Verified the fix specifically — added a web app, then triggered a full
  rescan via `--restart`, confirmed it survived.
- **Settings window text was unreadable.** Root cause: WPF's default `TabItem`/
  `TabControl`/`ComboBox` chrome uses Windows system theme colors for header text
  and the content-area background, ignoring the window's own `Foreground`/
  `Background` — against this app's dark theme that rendered as invisible
  dark-on-dark or white-on-white text. Fixed with explicit custom templates for all
  three controls (`SettingsWindow.xaml`) rather than relying on property
  inheritance, which doesn't reach into default chrome. This was verified with an
  actual screenshot of the rendered window (all four tabs), not just UI Automation
  tree inspection — tree inspection reports the right *text*, it can't tell you
  whether that text is visible against its background, which is exactly what this
  bug was.
- **Date/time format**: changed to `h:mm tt` (12-hour) and `ddd, dd MMMM yyyy`
  (e.g. "Mon, 17 August 2026") per explicit request, replacing the previous
  hardcoded 24-hour/short-date format. A Settings toggle for 12h/24h (Section 15)
  remains a reasonable follow-up rather than done here.
- **Methodology note**: earlier phases relied on UI Automation's accessibility tree
  for verification, which reports control *structure and text content* reliably but
  says nothing about *visual rendering* — exactly the gap that let the Settings
  text-visibility bug ship unnoticed through Phase 7's testing. This pass used
  actual screen captures (`Graphics.CopyFromScreen` against real `GetWindowRect`
  physical-pixel coordinates — UI Automation's own `BoundingRectangle` proved
  unreliable for this across sessions, consistent with the DPI-virtualization notes
  in Phase 6/8) to catch what tree-only verification couldn't.

## Round 2 polish pass (second batch of real usage feedback)

A second round of feedback came in after further day-to-day use. All seven items
addressed; verification status is called out per item since two items could not be
fully live-tested in this sandboxed dev environment for reasons explained below (not
because the code is untested — `dotnet build`/`dotnet test` both pass clean).

- **Running-window icons bigger, with margin.** `BuildIconContent`
  (`TaskbarWindow.xaml.cs`) changed from a bare 16x16 `Image` to 22x22 with
  `Margin="4,0,4,0"`. Live-verified via screenshot.
- **Custom category icon from disk (PNG/ICO/SVG), saved independently.** New
  `Applications/CategoryIconService.cs`: rasterizes any of the three formats (SVG via
  the new `Svg` NuGet package; PNG/ICO via `System.Drawing.Bitmap`, already available
  through the WinForms reference) down to a fixed 64x64 and writes the app's own PNG
  copy under a new `AppPaths.CategoryIconsDirectory` (`%APPDATA%\FlexTaskbar\
  CategoryIcons\{categoryId}.png`) — never a reference back to the file the user
  picked, so moving/renaming/deleting the original can't break the category's icon.
  `ApplicationCategory.CustomIconPath` stores the result; `CategoryManager.
  SetCustomIcon` wires it in and clears it if the user later picks an emoji glyph
  instead (and vice versa). `CategoryManagementWindow` gained a "Choose from file...
  (PNG/ICO/SVG)" button next to the existing emoji picker. Build/test verified only —
  not live-clicked through in this pass; the risk is low since it reuses the same
  `OpenFileDialog` + try/catch pattern already proven elsewhere in this codebase.
- **Mirror the real Windows system tray's icons.** New
  `Services/TrayMirrorService.cs`, rendered in a new `TrayMirrorPanel` next to the
  clock. This one took real debugging to get right, worth recording:
  - UI Automation (this project's usual accessibility API of choice) turned out not
    to expose the tray's individual icon buttons at all in this environment — walking
    into the `ToolbarWindow32` ("User Promoted Notification Area") returned zero
    children no matter how it was queried. Switched to MSAA/`IAccessible`
    (`Native/Oleacc.cs`) instead, which classic toolbar controls implement natively —
    the same technique long used by third-party tray utilities, and still a public,
    documented COM interface (not memory-reading).
  - `FindWindowEx` proved unreliable for locating the toolbar child window in this
    environment (failed even with no filter at all), while `EnumChildWindows`
    reliably found the same window moments later — switched to that instead. Both
    are documented inline in `Native/User32.cs` since the failure mode was
    surprising enough to be worth recording for future readers.
  - The `IAccessible` COM interface itself needed `InterfaceType(ComInterfaceType.
    InterfaceIsDual)`, not `InterfaceIsIDispatch` — IAccessible's IDL marks it
    `dual`, and late-bound `IDispatch::Invoke` calls (what `InterfaceIsIDispatch`
    produces) failed with `DISP_E_MEMBERNOTFOUND` against the real tray's accessible
    object even with correct DISPIDs. Vtable-based early-bound calls (what
    `InterfaceIsDual` produces) work correctly — but require every interface member
    to appear in the exact order oleacc.idl declares them, since vtable slots are
    positional.
  - **Live-verified**: with diagnostic logging temporarily enabled, confirmed the
    service correctly enumerated all 13 real tray items on this dev machine (by
    name: FlexTaskbar itself, Rainmeter, OpenMultipleApps.exe, Windhawk, Speakers,
    a Wi-Fi/Tailscale network icon, PowerToys, Everything, AdGuard, Bluetooth
    Devices, plus 2 unnamed) and correctly mirrored the 2 that had real on-screen
    positions (Speakers, network) into `TrayMirrorPanel` — confirmed via screenshot
    crop showing both icons rendered next to the clock. The other 11 all had an
    empty bounding rect, because they're currently tucked behind this machine's "^"
    hidden-icons overflow rather than always shown — see the next point.
  - **Documented, deliberate limitation**: only icons with a real on-screen position
    are mirrored. Icons hidden behind the "^" overflow chevron are not, because
    reading them would require the service to click that chevron open against the
    real taskbar — a visible, disruptive action against the user's actual desktop
    that this service intentionally avoids. On a machine with Windows' "always show
    all icons" setting on (which is what the original feedback describing "10+ icons
    already visible" implies), every icon they meant would already have a real
    position and would be mirrored.
- **Start, All, Link each get their own button.** The "+ Web App" button (feature
  internally called "Link") moved out of the "All Applications" popup header and
  into its own top-level `LinkButton` next to `StartButton`/`AllAppsButton`, reusing
  the existing `AddWebAppButton_Click` handler unchanged. Live-verified via
  screenshot.
- **Category buttons now blend with the app-icon visual language.** Replaced the
  `"📁 Name ▾"` text label with `BuildCategoryIconContent` — a small `Grid` matching
  the running-window buttons' 22x22 icon sizing, showing the category's custom image
  icon if set (`CategoryIconService.TryLoadIcon`) or its emoji glyph otherwise, with
  a small "▾" corner overlay so it's still visually distinguishable as a menu
  trigger. The category name moved to the button's tooltip and
  `AutomationProperties.Name` instead of being drawn inline. Not live-tested this
  pass (no categories existed in the fresh test profile used for this pass) — build
  verified only.
- **Launcher window's Settings button wasn't working.** Root cause: it was a
  deliberately disabled Phase-5 placeholder (`SettingsWindow` didn't exist until
  Phase 7) that never got rewired afterward — a genuine regression, not a new
  feature. `LauncherWindow` now takes an `Action openSettings` constructor parameter
  (wired from `TaskbarWindow.OpenLauncher()` to the existing `OpenSettings()`
  method), and a real `SettingsButton_Click` handler calls it then closes the
  launcher. Live-verified: clicked Start → Settings in the running app and confirmed
  via window enumeration that a real, visible `SettingsWindow` was created
  (`IsWindowVisible` true, correct bounds) — a full end-to-end proof the wiring
  works. Could not get an unobstructed screenshot of its *contents* in this specific
  pass because an unrelated always-on-top window already present in this sandboxed
  session's desktop (not a FlexTaskbar window) wouldn't cede the foreground to
  `SetForegroundWindow`, even after the standard Alt-key-press workaround — a test
  environment limitation, not a FlexTaskbar issue (the Settings UI's actual
  appearance was already screenshot-verified in the prior polish pass, and nothing
  in this change touches its content, only how it's opened).
- **Replaced the "+" (New Category) button with a right-click menu item.** Removed
  `NewCategoryButton` from the taskbar's left panel entirely; added a "New
  Category..." entry to the existing right-click context menu
  (`Background_MouseRightButtonUp`), calling the same `NewCategoryButton_Click`
  logic. Button removal live-verified via screenshot (no "+" present). The new menu
  item itself was not live-verified — the right-click context menu did not render
  visibly in a screenshot during this pass, most likely obscured by the same
  always-on-top window noted above rather than a real bug (it reuses the exact
  event-registration pattern as the already-working "Settings..."/"Recovery..."
  items two lines above it in the same method). Worth a follow-up live click-test
  in a cleaner environment.

## Round 3 polish pass (third batch of real usage feedback)

- **Pin app shortcuts to the taskbar — not just running apps — including drag and
  drop.** `ApplicationEntry.IsPinned` had existed since early phases (persisted,
  round-tripped through backup/restore, even covered by a test) but nothing ever
  set it — a dead field. Now wired up end to end:
  - `ApplicationManager.GetPinnedApplications()`/`SetPinned()` — new, simple.
  - `ApplicationManager.PinShortcutFile(path)` — new; resolves a dropped .lnk via
    the existing `ShellIntegration.ResolveShortcut` (same COM interop
    `ApplicationScanner` already uses) or accepts a .exe directly, and pins it —
    creating a new entry if the target isn't already known, or just flipping
    `IsPinned` on a matching existing one (matched by the same SHA1-of-normalized-
    path `Id` scheme `ApplicationScanner`/`AddManualWebApp` already use, so a
    dropped shortcut to an app the scanner already found collapses into that same
    entry rather than duplicating it).
  - `TaskbarWindow`'s center panel (`RefreshRunningWindowButtons`) now renders
    pinned apps: as a normal running-window button if currently running (matched
    by executable path against `_runningWindows`), or a dimmed (`Opacity 0.55`)
    "click to launch" button otherwise — never a duplicate of both.
  - Two ways to pin: (1) drag an entry from the "All Applications" popup, or a real
    `.lnk`/`.exe` file from File Explorer/the desktop, onto anywhere on the
    taskbar (`Taskbar_DragEnter`/`Taskbar_Drop` on the outer `Border`, accepting
    both the existing internal `typeof(ApplicationEntry)` drag format and
    `DataFormats.FileDrop`); (2) right-click "Pin to taskbar" on an entry in the
    "All Applications" popup, or on any running-window button (added to
    `BuildWindowContextMenu`/`BuildGroupedWindowContextMenu`, which needed to stop
    being `static` to reach `_applicationManager`).
  - Build/test verified. Not live-clicked through in this pass (see the note at
    the end of this section on why UI-automation click-testing was cut short this
    round) — but every piece reuses an already-proven mechanism from earlier
    phases (the exact `DragDrop.DoDragDrop`/`typeof(ApplicationEntry)` format
    `CategoryButton_Drop` already used successfully; the exact context-menu-via-
    code-behind pattern used everywhere else in this file), so the risk is low.
- **Root category icons also shown in the center panel**, not just the left
  `CategoriesPanel`. Factored the button-building code that used to live inline in
  `RefreshCategoryButtons` out into a shared `BuildCategoryButton`, called from
  both `RefreshCategoryButtons` (left panel, unchanged appearance) and the now-
  category-aware `RefreshRunningWindowButtons` (center panel, new). Both panels
  now repaint on `_categoryManager.Changed` and `_applicationManager.Changed`
  (needed so a pin/unpin or new category shows up immediately in the center panel
  too, not just after the next window-open/close event). Build/test verified.
- **Clicking a multi-window app's icon shows a tile-based picker of its open
  windows**, replacing the previous behavior (guessing which window to activate —
  last-active, or first alphabetically). `ShowHoverWindowList` (built for the
  earlier "hover shows a list upward" feedback) was redesigned from a vertical
  list of plain text buttons into a `WrapPanel` of icon+title tiles, and
  `BuildGroupedWindowButton`'s `Click` now opens it instead of directly
  activating a window — hovering still opens the same popup unchanged. Each tile
  reuses `BuildWindowContextMenu` for its own Restore/Minimize/Maximize/Close/
  Pin actions. Build/test verified.
- **Tray mirror now includes icons hidden behind the "^" overflow chevron** — the
  literal apps named in the feedback (Claude, AutoHotkey, PowerToys, Windhawk,
  Rainmeter) were all confirmed present via live testing. This needed two
  attempts:
  1. First attempt extended the existing MSAA/`IAccessible` screen-capture
     approach from round 2: locate the chevron (a `SIBTrayButton` window — there
     are three, disambiguated by `IAccessible.get_accName(CHILDID_SELF)`; the
     chevron's own accessible name turned out to be *"Show hidden icons"*, not
     *"Notification Chevron"* as an earlier UI Automation dump had suggested —
     those are two different, unrelated labels), invoke its `accDoDefaultAction`
     to open the `NotifyIconOverflowWindow` flyout (a real "activate this
     control" call, not a synthetic mouse click — the cursor never moved), scan
     its toolbar the same way as the main strip, then invoke the chevron again to
     close it. This successfully found and named all 8 hidden icons on the first
     real test — but every captured image came back as a solid black square, even
     after adding a render-settle delay. Held the flyout open with a long debug
     delay and screenshotted it directly: **the flyout wasn't visually rendering
     at all** — `accDoDefaultAction` had toggled *some* internal state (the window
     existed, its toolbar was queryable, icon names/positions were real) without
     the OS actually compositing the popup to screen. Likely a DWM
     composition-timing quirk specific to a programmatically-invoked (vs.
     user-clicked) flyout on this Windows build.
  2. Rather than escalate to a real synthetic mouse click (moving the user's
     actual cursor, and doing so on every periodic refresh, was judged too
     disruptive for a background service), switched approach entirely: Windows
     records every app that has ever registered a tray icon under
     `HKEY_CURRENT_USER\Control Panel\NotifyIconSettings` (the same data Settings
     > Personalization > Taskbar > "Select which icons appear on the taskbar"
     reads), one subkey per icon with an `ExecutablePath` value. That list is a
     historical accumulation (every version of every app ever run, including
     long-uninstalled ones — confirmed live: dozens of stale entries for old
     Claude/Rainmeter versions), so `TrayMirrorService` cross-references it
     against currently-**running** processes (`Process.GetProcesses()`) and only
     shows an icon for a registry entry whose path matches a real running
     process's path (comparing suffixes, since registry paths are often prefixed
     with a known-folder GUID token like `{6D809377-...}\WindowsApps\...` rather
     than a real drive path). Each match gets its icon via the existing
     `ApplicationIconService`/`SHGetFileInfo` extraction path — the same one used
     for every other icon in the app — rather than a screen crop. This is simpler,
     causes zero visible disruption, needs no window automation at all, and (being
     a real extracted icon rather than a small rendered crop) is arguably
     higher-fidelity. Removed the now-unused `Native/Oleacc.cs`, `Native/Gdi32.cs`,
     and the `EnumChildWindows`/`GetClassName`/`FindWindowEx` additions to
     `Native/User32.cs` that only the abandoned approach needed.
  - **Live-verified**: screenshot after the rewrite shows real, distinct icons
    (AdGuard's shield, a magnifier for Everything, PowerToys' colored-squares
    logo, a folder icon, etc.) rendered in the tray-mirror panel — a marked
    improvement over both the round-2 version (2 icons, main strip only) and the
    abandoned flyout-automation attempt (solid black squares).
  - Scanning now runs on a background thread (`Task.Run`, guarded by a
    `_scanning` flag against overlapping ticks) rather than the UI thread, since
    enumerating every running process isn't free — this was already necessary
    for the abandoned flyout approach and was kept since it's good practice
    regardless.
- **On live-testing depth this round**: the tray-mirror rewrite got a full
  live-verification cycle (it was the one genuinely uncertain, exploratory piece).
  The other three items reuse mechanisms already proven working in this exact
  codebase in earlier rounds (drag/drop format, context-menu construction,
  popup-based window pickers), and an attempt to click through them via
  synthetic mouse coordinates in this sandboxed session repeatedly missed the
  intended buttons (coordinates drifted between screenshots) without surfacing
  any actual defect — so this pass stopped at build+test verification for those
  three rather than burn further time chasing pixel-perfect synthetic clicks.
  Worth a real click-through pass next time a human is at the keyboard.

## Round 4 polish pass (fourth batch of real usage feedback)

- **Start button now shows the app's own icon, not just wording.** Was
  `Content="◉ Start"` (an emoji glyph standing in for an icon). Now a
  `StackPanel` of `<Image Source="pack://application:,,,/Resources/App.ico">` +
  a "Start" `TextBlock`, matching how every other icon+label control in the app
  is built. `AutomationProperties.Name="Start"` set explicitly since composite
  content can't be auto-derived (same reason this is done everywhere else in
  this codebase). Live-verified via screenshot.
- **Removed the left-side category icons — center panel only.** The left
  `CategoriesPanel` (added in round 2, then made icon-based) duplicated the
  category icons already shown in the center panel since round 3. Removed the
  XAML element, the now-dead `RefreshCategoryButtons` method and its
  `_categoryManager.Changed` subscription — `BuildCategoryButton` now only ever
  gets called from the center panel's `RefreshRunningWindowButtons`. Live-verified
  via screenshot (no separate category cluster left of "All"/"Link").
- **Tray-mirrored icons can be right-clicked for that app's real context menu.**
  There's no way to synthesize a Shell_NotifyIcon context menu ourselves — it's
  owned and drawn by the icon's own app (or Explorer) — so `TrayMirrorService.
  TryShowContextMenu(name)` locates the actual real icon (MSAA/`IAccessible`
  toolbar scan, same technique as the abandoned round-2/3 screen-capture
  approach, but now used only for live position lookup, on-demand, not
  periodically) and forwards a *real* right-click there (`SetCursorPos` +
  `mouse_event`), first checking the always-visible strip, then opening the "^"
  overflow flyout and checking there if not found (closing the flyout again only
  if no match was found — closing it after a successful click would cancel the
  context menu just triggered). Re-added `Native/Oleacc.cs` (deleted in round 3)
  since this needed the same IAccessible plumbing back, now solely for this
  on-demand lookup rather than periodic scanning.
  - Name matching is a best-effort case-insensitive substring check between the
    registry-derived display name (e.g. "Everything") and the real icon's live
    MSAA accessible name/tooltip (e.g. "Everything" or "PowerToys v0.97.2") —
    these come from different sources (see round-3 notes) and won't always agree
    exactly; a miss is a harmless no-op, never a wrong guess.
  - **Known, inherent UX quirk, not a bug**: the context menu appears at the
    *real* tray icon's actual location (bottom-right corner, wherever the real
    Explorer taskbar is), not at the mirrored icon's position on FlexTaskbar's
    own bar — unavoidable, since triggering it means clicking the real icon.
  - **Partially live-verified**: confirmed end-to-end that right-clicking the
    mirrored "Everything" icon correctly matched the real tray icon (the real
    mouse cursor visibly relocated to it and its tooltip appeared, proving the
    lookup and click-forwarding pipeline both ran correctly), but couldn't
    confirm the resulting context menu's on-screen appearance within this test
    pass. Build/test verified regardless; worth a direct human check.
- **"Move to Category" menu added to the "All Applications" popup** (round-4
  feedback: "cannot add apps into parent category from the All menu — how to add
  apps into that category?"). The only previous way was dragging an entry from
  that popup onto a category icon — real, working, but entirely undiscoverable
  without already knowing it existed. `AppsListBox_MouseRightButtonUp`'s existing
  Pin/Unpin menu now also gets a "Move to Category" submenu (including
  "Uncategorized"), built from `CategoryManager.GetAllFlattened()` — a method
  that had existed since an early phase specifically for this kind of picker
  (its doc comment says so) but was never actually wired to any UI until now.
  Build/test verified.
- **Fixed black, barely-visible text in Settings' checkboxes/radio buttons.**
  Same root cause as the earlier TabItem/ComboBox fix (documented in the
  post-Phase-9 section above): the implicit `CheckBox`/`RadioButton` styles only
  had a `Foreground` **Setter**, not a custom `ControlTemplate` — and the default
  WPF template's label text doesn't reliably bind to that Setter against every
  Windows theme, rendering as near-invisible dark-on-dark text exactly as
  reported. Added explicit templates (a themed box/circle + a checkmark `Path`
  or dot `Ellipse` shown only when checked, plus a `ContentPresenter` with
  `TextElement.Foreground` bound to the app's theme brush) for both controls, the
  same fix pattern as TabItem. Also fixed one unrelated but same-symptom bug found
  while auditing for this: `CategoryManagementWindow.xaml`'s "Close" button had no
  `Style="{StaticResource TaskbarButtonStyle}"` at all, unlike its siblings.
  Live-verified via screenshot — "Start with Windows"/"Auto-hide taskbar"/"Reserve
  screen space" labels are now clearly legible white-on-dark text.

## Round 5 polish pass (fifth batch of real usage feedback)

- **Tray icon right-click genuinely didn't work — fixed, but scaled back from
  round 4's design.** Live debugging (with temporary diagnostic logging) traced
  it through several layers:
  1. The click handler itself, and the main-strip/overflow lookup, were all
     firing correctly — but `SendRightClick`'s `mouse_event`-based click (round 4)
     produced no visible effect even when it found the right icon. Switched to
     `SendInput` (the API Microsoft actually recommends over the legacy
     `mouse_event`), plus a settle delay after `SetCursorPos` before clicking.
  2. Testing then showed most real tray icons in this environment aren't in the
     always-visible strip at all — they're behind the "^" overflow chevron
     (`rect=0,0,0,0` in the main strip, real coordinates only once the overflow
     is queried). Round 4's overflow path invoked the chevron via MSAA's
     `accDoDefaultAction`; live testing proved that creates the
     `NotifyIconOverflowWindow` and makes its toolbar queryable (real names, real
     non-zero rects) **without actually compositing the flyout to the screen** —
     the identical failure mode already documented for round 3's abandoned
     screen-capture attempt. Clicking at those "real" coordinates therefore hit
     whatever was actually on screen at that point instead.
  3. Switched the chevron open/close to a genuine synthetic click (`SendInput` at
     the chevron's own `GetWindowRect`) instead of the MSAA invoke, reasoning
     that going through the same input pipeline as a real user would reliably
     produce a real, interactive flyout. Live testing showed this was **worse,
     not better**: on one run it correctly found and right-clicked "Everything"
     in the (apparently now-open) overflow flyout; the resulting screenshot
     instead showed the desktop had switched to Windows Settings' Date & time
     page — meaning the synthetic click sequence hit something real and
     unrelated, not the intended tray icon menu at all.
  - Given a harmful, unpredictable side effect (clicking something the user
    never asked for) is a worse failure mode than the feature simply not
    working, **the overflow-flyout path was removed entirely** rather than
    shipped in that state. `TryShowContextMenu` now only ever forwards a click
    to icons already in the always-visible strip (still using the `SendInput`
    fix from point 1, which is real and correct on its own) — icons hidden
    behind the chevron are a clean, honest no-op; today's only option for those
    is the real system tray's own "^". All the now-unused MSAA overflow/chevron
    code (`FindChevronHandle`, `GetAccessibleSelfName`, `ClickChevron`,
    `WaitForWindow`) was deleted rather than left as dead code.
  - Not independently re-verified against a real always-visible icon in this
    pass (this environment's icons are all behind the chevron) — the `SendInput`
    fix itself is a reasonable, standard-practice improvement even so.
- **FlexTaskbar's own tray icon now appears in its own tray mirror.** The
  round-2/3 self-exclusion (`if (path == _selfExecutablePath) continue`) was
  removed per direct request — the real system tray shows FlexTaskbar's icon
  like any other app's, so the mirror should too. Live-verified via screenshot.
- **Settings window's title bar text was still black — root cause was different
  from the checkbox/radio bug fixed in round 4.** `SettingsWindow.xaml` was the
  only window in the app that never set `WindowStyle="None"` — it was still
  using the real **OS-drawn title bar**, which WPF does not theme automatically;
  it renders using whatever chrome the current Windows theme happens to use,
  independent of this app's own dark theme. Round 4's CheckBox/RadioButton fix
  was real but addressed a different bug in the same window, which is why the
  report persisted. Fixed at the root by switching to `WindowStyle="None"` with
  a self-drawn header (icon + "FlexTaskbar Settings" + a themed ✕ button, plus
  `DragMove()` wired to the header so the window is still draggable) — the same
  approach `TaskbarWindow`/`LauncherWindow` already use, and inherently
  theme-proof since it's just our own XAML rather than OS chrome. Also made the
  `SectionHeader` style (used for "Applications"/"Position"/etc.) explicitly
  `BasedOn` the base themed `TextBlock` style — it's an `x:Key` style, which
  doesn't automatically inherit an implicit same-`TargetType` style's setters
  just by targeting the same element type, so this closes a latent gap even
  though property-value inheritance from the Window likely papered over it
  before. Live-verified via screenshot — custom dark-themed header, clearly
  legible "FlexTaskbar Settings" title.
- **Web apps can now be added via a raw executable+arguments command, or a
  `shell:AppsFolder\{AUMID}` launch** — not just a plain URL. Directly answers
  the two examples given (a Chrome PWA launched via `chrome_proxy.exe
  --profile-directory=... --app-id=...`, and Discord's installed PWA launched
  via `explorer.exe "shell:AppsFolder\discord.com-...!App"` — though the
  `explorer.exe` prefix turns out to be unnecessary: `ShellExecute` opens a
  bare `"shell:AppsFolder\..."` target directly). New
  `ApplicationLaunchKind.ShellCommand` for the latter (handed straight to
  `ProcessStartInfo` with `UseShellExecute = true`, skipping the
  `File.Exists` check that doesn't make sense for a virtual shell path) and
  `ApplicationManager.AddManualShortcut(name, executablePath, arguments)` for
  both forms. The existing "Link" button's flow now detects which of three
  forms the user entered (http(s) URL → existing `AddManualWebApp`; `shell:`
  prefix → `AddManualShortcut` with no arguments prompt; anything else →
  `AddManualShortcut` with a further optional-arguments prompt) rather than
  assuming URL-only. Deliberately two separate prompts (path, then arguments)
  rather than one pasted command-line string — splitting an unquoted path that
  can itself contain spaces (e.g. `C:\Program Files\Google\Chrome\...`) from
  its arguments is unsolvable in general without that separation. Build/test
  verified; not live-clicked through this pass (multi-step prompt sequences
  don't survive this session's synthetic-click coordinate issues well — see
  round-3/4 notes on the same limitation), but the implementation directly
  reuses `AddManualWebApp`'s already-proven id-dedup/persist/notify pattern.

## Round 6 polish pass (sixth batch of real usage feedback)

- **Tray icon right-click — genuine further progress, landed on an honest final
  design after four rounds of attempts.** Live debugging (temporary diagnostic
  logging again) found two real bugs on top of round 5's `SendInput` fix:
  1. `TrySelectAndOpenMenu`'s icon lookup matched by name only, without
     checking whether that icon was actually laid out in the toolbar being
     searched. Every icon is an accessible child of the **main strip's**
     toolbar at all times — even ones actually hidden behind the chevron —
     just with a `0,0,0,0` location when not really there. Without a
     visibility check, a name match in the main strip always "won" even for a
     hidden icon, so the overflow flyout was never even tried. Fixed by
     gating on `accLocation` returning a non-zero rect (used only as a
     visibility filter now, never to compute a click coordinate).
  2. With that fixed, live testing reached a real milestone: the overflow
     flyout genuinely opened, and the correct icon visibly received a focus
     rectangle (`IAccessible.accSelect`) — the first time any attempt across
     rounds 4–6 got this far without misfiring. But the synthetic "Menu" key
     sent afterward still opened nothing. Added `SetForegroundWindow` before
     the keystroke (accessibility-level MSAA focus isn't the same as real
     Win32 keyboard focus, so the key was likely being delivered back to
     FlexTaskbar itself) and a Shift+F10 fallback — neither made the menu
     appear. Given three consecutive rounds of attempts at fully automating
     this specific gesture, with round 5 having already produced one actual
     wrong-click side effect, continuing to guess was judged not worth the
     risk. **Final design**: for an icon in the always-visible strip, the
     full accSelect + SetForegroundWindow + Menu-key sequence is still
     attempted (no evidence it's unsafe there — the failures were specific to
     the overflow flyout). For an icon behind the chevron, the real flyout is
     opened and the icon is accSelect-highlighted, then deliberately left
     open for the user's own next click — smaller than full automation, but
     reliable and never wrong. Live-verified: right-clicking a mirrored
     overflow icon now cleanly reveals the real, correctly-populated flyout
     every time, with no misfires across repeated testing.
- **Tray mirror icons made smaller** (14x14, down from 18x18) so they read as
  visually distinct from the 22x22 running/pinned app icons, matching how the
  real Windows tray renders noticeably smaller than taskbar buttons.
  Live-verified.
- **Wider margin between Start/All/Link.** The shared `TaskbarButtonStyle`'s
  default 4px trailing margin was judged insufficient by direct feedback for
  these three specifically; gave `StartButton`/`AllAppsButton` an explicit
  10px trailing margin (overriding the style default) rather than changing it
  globally, since the tighter default margin is intentional for icon-only
  category/pinned-app buttons elsewhere on the bar. Live-verified.
- **"Choose from file" for a category icon did nothing — root cause was an
  exception silently swallowed by the app-wide safety net, not a missing
  feature.** `ChooseIconFile_Click`'s catch clause only handled
  `IOException`/`UnauthorizedAccessException`/`NotSupportedException`; image
  decoding (`System.Drawing.Bitmap`'s constructor for PNG, the `Svg` library
  for SVG) can throw other types for anything it doesn't like — `ArgumentException`
  chief among them — which fell through to `App.xaml.cs`'s
  `DispatcherUnhandledException` handler instead, logging to `Console.Error`
  with no visible feedback to the user at all. Widened the catch to plain
  `Exception` so any failure now surfaces as the warning dialog that was
  always intended. Separately, actually fixed `.ico` handling rather than
  just reporting its failure better: `System.Drawing.Bitmap`'s own
  constructor doesn't reliably load the multi-resolution `.ico` container
  format — every `.ico` pick was silently failing before this pass regardless
  of the catch-clause bug. `CategoryIconService.RasterizeIco` now loads it via
  `System.Drawing.Icon` (which does understand the format) and converts to a
  bitmap from there. Build/test verified; not re-triggered live this pass.

## Round 7 polish pass (seventh batch of real usage feedback)

- **"Choose from file" — actually fixed this time.** Round 6 widened the catch
  around `_categoryManager.SetCustomIcon(...)`, reasoning the image-decoding
  step was throwing something uncaught — real, but not the whole story.
  `dialog.ShowDialog(this)` itself was sitting **outside** that try/catch
  entirely. `OpenFileDialog.ShowDialog` can throw (a `COMException` from the
  Windows shell item picker was observed in this environment, though it's not
  necessarily the only way), and with no catch around that specific call, the
  exception had nowhere to go but the same app-wide
  `DispatcherUnhandledException` safety net as before — invisible to the user,
  identical symptom, one call earlier than where the previous fix reached.
  Wrapped the entire method body this time, dialog creation and
  `ShowDialog` included. **Live-verified**: seeded a test category, opened
  Manage Category, clicked "Choose from file...", and confirmed via window
  enumeration that the real "Choose a category icon" file picker now reliably
  opens — something that never happened in any previous testing attempt across
  rounds 2, 6, or 7's first pass.
- **Web apps added via Chrome/Edge PWA shortcuts were silently vanishing from
  the app list (and therefore search) — a real duplicate-id bug, not a search
  bug.** `ApplicationScanner`'s `Id` was computed from the shortcut's target
  exe path alone. Ordinary apps: fine, two shortcuts to the same exe should
  collapse into one entry (Section 36's intentional duplicate detection). But
  Chrome/Edge PWA shortcuts all point at the exact same launcher
  (`chrome_proxy.exe`/`msedge_proxy.exe`) and are distinguished *only* by a
  `--app-id=...` argument — so every installed PWA collapsed onto the same Id,
  and the scanner's results dictionary (keyed by Id) silently kept only the
  last one scanned. Every other web app was gone before the app list or search
  ever saw it — not a filtering bug, the entries genuinely never existed past
  the scan. Fixed by folding `Arguments` into the id seed
  (`ApplicationScanner.BuildIdSeed`) whenever present — apps with no arguments
  (the vast majority) hash identically to before, so this doesn't disturb
  existing persisted ids for ordinary apps. Applied the same fix to
  `ApplicationManager.AddManualShortcut`/`PinShortcutFile` (round 5/3's manual-
  add paths), which had the identical bug for manually-entered PWA commands.
  `ApplicationSearch` itself needed no change — it already searched the full
  `Applications` collection by name; the entries just weren't in that
  collection to begin with. One-time consequence worth flagging: on the next
  rescan, previously-collapsed PWA entries will split into their correct
  individual entries, but since they were never distinguishable before, any
  pin/category assignment on the old collapsed entry can't be cleanly migrated
  to "the right one" — affected users will need to re-pin/re-categorize their
  PWAs once. Build/test verified; not re-scanned live this pass (no PWA
  shortcuts present in this test environment to reproduce against).
- **Tray icon right-click redirecting to "Windows' default system tray
  menu"**: this is round 6's deliberate final design working as intended, not
  a bug — see that section above for the full reasoning (four rounds of
  attempts established that fully automating a hidden icon's menu trigger
  isn't reliably achievable without risking a wrong click). Flagged back to
  the user directly for a product decision on whether to keep the "reveal the
  real tray flyout" behavior, replace it with no interaction at all, or
  something else, rather than continue iterating blind on the same
  four-times-attempted mechanism.

## Round 8: AppBar reservation compounding-shrink bug

User-reported, outside the numbered feedback rounds above: enabling "Reserve
screen space (AppBar)" caused other windows' usable area to keep shrinking
over time instead of settling at a fixed strip — "it not stay on the bottom."

**Root cause**: `TaskbarLayoutManager.ApplyPosition()` computed the reserved
rectangle from `MonitorService.GetPrimary().WorkArea` queried fresh on every
call. That's correct exactly once — the first time, before FlexTaskbar has
reserved anything. On every subsequent call, though, `WorkArea` already
reflects FlexTaskbar's *own* prior reservation (Windows' work-area query
necessarily includes every currently-registered AppBar, ours included). Docking
to `WorkArea.Bottom - BarThickness` using that already-shrunk value claims an
*additional* slice on top of the last one. `ApplyPosition()` is called
repeatedly in normal operation — most importantly by `TaskbarWindow`'s 20-second
AppBar health-check timer (`HandleShellRestarted()`, added in Phase 8 as
defense-in-depth against Explorer forgetting the registration across its own
restarts — see that phase's notes above) — so the reservation grew a little
more every 20 seconds, indefinitely, exactly matching the reported symptom.

**Fix**: `TaskbarLayoutManager` now captures the work area **once**, in
`Register()`, into a `_baselineWorkArea` field, before `ABM_NEW`/`ApplyPosition`
claim any space — the one moment `WorkArea` is guaranteed to reflect every
*other* AppBar (the real Windows taskbar included) but not FlexTaskbar's own.
`ApplyPosition()` now anchors to that cached baseline instead of re-querying.
A second, related bug in the same fix: `HandleShellRestarted()` used to route
through `Register()`, which — after this change — would have re-captured the
baseline on *every* call, including the unconditional 20-second health-check
ticks (not just genuine restarts), reintroducing the identical compounding
bug through a second path. Split `Register()`/`HandleShellRestarted()` into a
shared `RegisterCore(captureBaseline)`: a genuine first-time (or post-
`Unregister()`) registration captures a fresh baseline; the periodic health-
check re-registration reuses the existing one, since only FlexTaskbar's own
registration with Explorer was forgotten across a restart — the *external*
work area it originally measured hasn't changed. `Unregister()` clears the
cached baseline so a later fresh `Register()` re-measures correctly rather
than reusing a stale value.

Build/test verified (22/22). Not re-verified live in this pass — observing
"does it stay fixed over several minutes with the health-check timer firing"
needs sustained real desktop use to confirm, and enabling/testing AppBar
reservation live was treated as needing explicit permission in earlier phases
(it changes shared desktop state for every other window) rather than something
to toggle on unprompted during automated testing. Worth confirming on your end
by re-enabling "Reserve screen space" and checking the reserved strip stays a
constant thickness over a few minutes rather than creeping upward.

## Round 9: category icon picker COM activation failure

User-reported: clicking "Choose from file..." in the category icon picker
immediately showed "Couldn't use that file as an icon: Retrieving the COM
class factory for component with CLSID... [REGDB_E_CLASSNOTREG]" — with the
file picker dialog never appearing at all (confirmed live: the error fires
before `ShowDialog()` returns anything, not during the icon-decoding step
round 7 already fixed the exception-handling around).

**Root cause**: `app.manifest` declared `supportedOS`/DPI-awareness entries
but never declared a dependency on the Common Controls v6 side-by-side
assembly. `Microsoft.Win32.OpenFileDialog` activates the modern shell file
picker via COM (`IFileOpenDialog`); without an activation-context entry
routing to the v6 comctl32/shell dialog components, COM class resolution for
that picker isn't guaranteed to succeed on every system — this is the
standard, widely-documented cause of exactly this "class factory... not
registered" failure for .NET file dialogs. Confirmed not caused by running
elevated, and not caused by launching via `dotnet run` instead of the native
.exe (both ruled out via direct questions) — a genuine manifest gap.

**Fix (attempt 1, insufficient)**: added the standard Common Controls v6
`<dependency>` declaration to `app.manifest`. Build/test verified (22/22),
but user confirmed live that the picker still failed after rebuilding — the
Common Controls v6 manifest gap was not the actual cause on this machine.

**Corrected root cause (Round 9 continued)**: the user provided the actual
error text this time — HRESULT `0x8007007E` (`ERROR_MOD_NOT_FOUND`, "The
specified module could not be found"), not `REGDB_E_CLASSNOTREG` ("Class not
registered") as originally assumed. Those are different failure modes: this
one means the COM class `{DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7}`
(`CLSID_FileOpenDialog`) *is* registered, but the DLL its registration points
to can't be loaded — i.e. a broken or missing Windows shell component on that
specific machine (the process implementing the modern `IFileOpenDialog`
picker), not anything about FlexTaskbar's manifest or code. Supporting
evidence: the user separately reported being unable to save screenshots from
Windows' own screenshot tool, which very likely goes through the same shared
shell dialog infrastructure — pointing at a system-wide Windows health issue,
not an app-specific one. No manifest or code change inside FlexTaskbar can
repair a broken system DLL.

**Fix (attempt 2 — code-level workaround)**: switched
`ChooseIconFile_Click` (`Menus/CategoryManagementWindow.xaml.cs`) from
`Microsoft.Win32.OpenFileDialog` to `System.Windows.Forms.OpenFileDialog`
with `AutoUpgradeEnabled = false`, which forces the legacy, non-COM
`GetOpenFileName`-based picker. That code path never touches
`IFileOpenDialog`/`CLSID_FileOpenDialog` at all, so it sidesteps whatever's
broken on the affected machine entirely, regardless of root cause. Added a
small `IWin32Window` adapter (`Win32WindowHandle`) so the WinForms dialog is
still properly owned/modal to the WPF window, matching the previous
`ShowDialog(this)` behavior. Build/test verified (22/22); not independently
reproducible in this sandboxed dev environment (the original error never
occurred here either), so still worth the user confirming the picker now
opens and saves an icon successfully on their machine.

**Not fixed by this change**: the underlying Windows shell health issue
itself (also responsible for the screenshot-saving failure) is outside
FlexTaskbar's control — the workaround only avoids the broken component
inside FlexTaskbar's own file picker. If the user wants that fixed system-wide
(so their screenshot tool and any other app hitting the same component also
recovers), standard Windows repair tools (`sfc /scannow`, then
`DISM /Online /Cleanup-Image /RestoreHealth` if that doesn't resolve it) are
the appropriate next step, run at the user's own discretion.

## Round 10: "All Applications" popup — drag-to-category and launch-on-click collided

User-reported: "both not working. When i click apps to drag and drop, it
directly open the apps." — both the drag-onto-category-button gesture and,
implicitly, the right-click "Move to Category" menu (hard to reach it at all
if any stray click launches the app and closes the popup first).

**Root cause**: `AppsListBox_SelectionChanged` was what launched the app and
closed the popup (`Taskbar/TaskbarWindow.xaml.cs`). But WPF's `ListBoxItem`
selects on mouse *down*, not mouse-up — so `SelectionChanged` fired, and the
app launched and the popup closed, the instant the mouse button went down,
before `PreviewMouseMove` ever accumulated enough movement to recognize a
drag gesture. A drag always lost the race to an instant "click" resolution.

**Fix**: moved the launch from `SelectionChanged` (now a no-op that just
clears the transient selection highlight) to a new
`AppsListBox_PreviewMouseLeftButtonUp` handler, and added a
`_appsListBoxDragging` flag set the moment `DragDrop.DoDragDrop` is invoked
in `AppsListBox_PreviewMouseMove`. Mouse-up only launches when a drag wasn't
already recognized for that gesture — so a plain click still launches
immediately, but a click-and-drag now gets far enough (a real move past
`SystemParameters.MinimumHorizontalDragDistance`/`...VerticalDragDistance`)
for `DoDragDrop` to actually start before anything closes the popup. Right-
click "Move to Category" wasn't independently broken — but wiring launch to
mouse-up rather than mouse-down should also make it easier to reach reliably,
since a stray left-click no longer closes the popup out from under a
follow-up right-click attempt.

Build/test verified (22/22); not live-verified in this pass — dragging from
a `Popup`-hosted `ListBox` onto another window's controls (the category
buttons) is exactly the kind of gesture that's fragile to test headlessly via
synthetic `SendInput`, so this is worth confirming directly: open Apps, drag
an entry onto a category button, and separately confirm right-click → Move to
Category still opens and applies correctly.

## Round 11: no way to launch a manual shortcut elevated

User asked how to add a taskbar link for `services.msc` opened as
administrator (referencing a PowerShell `Start-Process -Verb RunAs` /
`ShellExecute("runas")` example). `ApplicationEntry.RunAsAdministrator` and
`ApplicationManager.Launch`'s `Verb = "runas"` wiring already existed, but
nothing in the UI ever set that flag — a real gap, not user error.

**Fix**: added `ApplicationManager.SetRunAsAdministrator` and a checkable
"Run as administrator" item to the "All Applications" popup's right-click
menu (`Taskbar/TaskbarWindow.xaml.cs`), next to Pin/Move to Category/Change
Icon. Toggling it persists via the existing `SaveCache()` path, same as
`SetPinned`.

To add the Services console: click **🔗 Link**, Name "Services", target
`mmc.exe`, arguments `services.msc`. Then open Apps, right-click "Services",
check "Run as administrator" — Windows will show its normal UAC prompt on
launch from then on (FlexTaskbar can't and shouldn't suppress that).

Build/test verified (22/22); not live-verified (elevation UI + a real UAC
prompt is exactly the kind of thing to confirm by hand, not synthetic input).

## Round 12: pinned custom icon showed the wrong app's icon for shared-exe entries

User-reported: after setting a custom icon on a manually-added "Services"
shortcut (`mmc.exe services.msc`), pinning it still showed the native mmc.exe
icon instead.

**Root cause**: `BuildPinnedAppButton` didn't render the entry it already had
— it re-resolved a "matching" entry by searching `_applicationManager
.Applications` for `ExecutablePath == entry.ExecutablePath` and rendered
*that* result's icon instead (`FindApplicationEntryByExecutable` /
`BuildIconContent(string? executablePath)`, `Taskbar/TaskbarWindow.xaml.cs`).
That match ignores `Arguments` entirely, so any two entries sharing the same
target exe — extremely common for `mmc.exe`-based admin tools (Services,
Event Viewer, Disk Management, Computer Management all launch via
`mmc.exe /s <name>.msc`) — collide onto whichever one happens to come first
in the list. The custom icon set on the actual "Services" entry could render
on a sibling entry instead, or not appear at all if the sibling that won the
lookup had no custom icon of its own.

**Fix**: `BuildIconContent` now takes the `ApplicationEntry` directly (the
caller already had it — the re-lookup was always redundant, just also
silently wrong) and calls `IconService.GetIcon(entry)` straight away. Removed
`FindApplicationEntryByExecutable`, since its only caller no longer exists and
the "Phase 4 running windows" fallback its doc comment described was already
removed in an earlier round (`BuildPinnedAppButton`'s own comment: "there's no
running/not-running distinction to render since the taskbar no longer tracks
open windows").

Build/test verified (22/22); not live-verified (needs two real pinned entries
sharing an executable — e.g. Services + Event Viewer, both via mmc.exe — to
reproduce and confirm the fix distinguishes them).

## Completed features

### Phase 1 — Foundation
- Solution (`FlexTaskbar.sln`) with `FlexTaskbar` (WPF app) and `FlexTaskbar.Tests` (xUnit) projects.
- Targets `net8.0-windows10.0.19041.0`, `SupportedOSPlatformVersion` `10.0.17763.0` (Windows 10 22H2-era baseline) — builds and runs on Windows 10 and 11.
- `app.manifest`: Per-Monitor V2 DPI awareness with legacy fallback, Windows 10/11 `supportedOS` GUIDs, `asInvoker` execution level (no admin required).
- `TaskbarWindow`: borderless, topmost window docked to the **work area** bottom edge of the primary monitor — i.e. it sits just above the real Windows taskbar rather than covering or replacing it. This is intentional: per spec Section 2, FlexTaskbar must run safely alongside the existing taskbar during development. True AppBar space reservation is Phase 6.
- Basic dark theme (`Resources/Theme.xaml`), Windows-11-flavored (flat, subtle borders, no heavy chrome).
- Live clock (`HH:mm` + short date), updated once per second via `DispatcherTimer`.
- Single-instance enforcement via named mutex (`Local\FlexTaskbar.SingleInstance.9F3B2C7A`).
- CLI parsing (`LaunchOptions`) recognizes `--safe-mode`, `--disable`, `--settings`, `--restart`, `--reset`. **Only `--safe-mode` and `--disable` currently short-circuit startup** (both just exit — no Settings/Recovery UI yet, since that's Phase 7/8). The other three flags parse but do nothing yet.
- Verified: `dotnet build` (0 warnings/errors), `dotnet test` (5/5 passing), and manual run — process launches, window renders, clock ticks, process exits cleanly.

### Phase 2 — Application discovery, icons, launching
- `Applications/ApplicationEntry.cs` — data model with stable `Id` derived from the
  *resolved target* (executable path or URL), not the shortcut path. This makes
  duplicate collapsing (Section 36) and preserving pin/category/elevation across
  rescans fall out for free: two shortcuts pointing at the same exe share an Id.
- `Applications/ApplicationScanner.cs` — async, bounded scan of Start Menu
  (`%ProgramData%` + `%AppData%`, `Programs` subfolder) and Desktop (`.lnk`, `.url`,
  `.exe`). Manual breadth-first directory walk (not `EnumerateFiles(..., AllDirectories)`)
  so one ACL-denied subfolder doesn't abort the whole scan. `.url` shortcuts are
  restricted to `http`/`https` schemes only (Section 40 — no `file://`/`javascript:`).
- `Native/Shell32.cs` + `Shell/ShellIntegration.cs` — `.lnk` resolution via
  `IShellLinkW`/`IPersistFile` COM interop (not `WScript.Shell` automation), per spec
  Section 64.
- `Applications/ApplicationIconService.cs` — icon extraction via `SHGetFileInfo` +
  `Imaging.CreateBitmapSourceFromHIcon` (pure WPF, no `System.Drawing` dependency),
  disk-cached as PNG under `%LOCALAPPDATA%\FlexTaskbar\IconCache\`, keyed by
  path + last-write-time so a replaced exe gets a fresh icon.
- `Applications/ApplicationManager.cs` — merges rescans with the live list (preserving
  `IsPinned`/`IsFavorite`/`CategoryId`/`RunAsAdministrator`), persists to
  `%APPDATA%\FlexTaskbar\applications.json` via `ConfigurationService`, and launches
  via `ProcessStartInfo` only — no `cmd.exe`/`powershell.exe`, `Verb=runas` only when
  `RunAsAdministrator` is explicitly set.
- `Services/ConfigurationService.cs` — atomic JSON read/write (`.tmp` → move) with a
  `.bak` fallback on corrupt/missing primary file (Section 28).
- Minimal "All Applications" popup wired into `TaskbarWindow` (an `Apps` button that
  lists every discovered app with its icon; clicking launches it) so discovery/icons/
  launching are visibly testable ahead of the real category system (Phase 3).
- **Verified on the actual dev machine**: scan found 200 real installed applications,
  correctly persisted to `applications.json`; build clean (0 warnings/errors) in Debug
  and Release; 8/8 tests passing.
- App icon: `Resources/AppIcon.svg` (editable source) rasterized to a multi-resolution
  `Resources/App.ico` (16–256px) via ImageMagick, wired into both `ApplicationIcon`
  (exe icon) and `TaskbarWindow.Icon`. Needed an explicit `<Resource Include=.../>`
  csproj entry — `ApplicationIcon` alone does *not* make a file reachable via
  `pack://application:,,,/`, and a stale incremental `obj/` cache masked this on the
  first attempted fix (a `dotnet clean` was needed to see the corrected resource list).

### Phase 3 — Categories, nested flyout menus, drag/drop
- `Applications/ApplicationCategory.cs` — category model; nesting is expressed via
  `ParentId` (unbounded depth) rather than an owned tree, so reparenting is a single
  field write and the flat list serializes simply.
- `Applications/CategoryManager.cs` — CRUD (add/rename/delete/move) backed by
  `categories.json` via the existing `ConfigurationService`. Deleting a category
  **never silently destroys data**: subcategories are promoted to the deleted
  category's parent, and apps in it become uncategorized rather than disappearing —
  this was a deliberate design call, not left to "whatever's easiest."
- `Menus/CategoryMenuBuilder.cs` — builds each category's flyout as a real, native
  `ContextMenu`/`MenuItem` tree (recursing into subcategories) instead of hand-rolled
  Popups. This gets submenu-on-hover, arrow-key navigation, and Escape-to-close for
  free from WPF's built-in `Menu` behavior (Sections 6 and 18), and reads as more
  "native Windows component" (Section 48) than a custom-drawn flyout would.
- `Menus/TextInputWindow.xaml(.cs)` + `Menus/CategoryManagementWindow.xaml(.cs)` — a
  real (not stubbed) "⚙ Manage Category" surface: Rename, New Subcategory, Delete
  (with confirmation), reachable from every category's flyout.
- `TaskbarWindow` now renders one button per **root** category dynamically
  (`📁 Development ▾`, etc.) plus an "All" button scoped to *uncategorized* apps only
  — "All" doubles as the drag source for sorting apps into categories.
- Drag/drop: dragging an app out of the "All" popup (`PreviewMouseMove` +
  `DragDrop.DoDragDrop`) onto a category button (`AllowDrop` + `DragOver`/`Drop`)
  calls `ApplicationManager.SetCategory`, which persists immediately and fires a
  `Changed` event the UI subscribes to for refresh.
- **Verified on the actual dev machine** via UI Automation (not just build success):
  launched the real app, drove the "+" → name-entry dialog → OK flow end-to-end,
  confirmed `categories.json` persisted correctly, confirmed the new
  `📁 Development ▾` button rendered on the taskbar, and confirmed clicking it opens
  a real native flyout menu containing "⚙ Manage Category". This process caught a
  **real crash bug**: `<InvariantGlobalization>true</InvariantGlobalization>` (added
  in Phase 1 as a size/perf optimization) turned out to crash WPF the instant any
  `TextBox` receives focus (`CultureNotFoundException` — WPF's caret/input-language
  handling needs real culture data). Removed it; see the csproj comment. Left in
  place, this would have shipped a taskbar where typing a category name crashed the
  whole app — exactly the kind of thing that's cheap to catch by actually running the
  app and expensive to catch any other way.
- **Not verified by automation, deferred**: actual mouse-driven drag-and-drop
  (`DragDrop.DoDragDrop` requires real native drag gestures — synthetic
  `InvokePattern` clicks can't drive it, and scripting raw `SendInput` mouse-drag
  reliably was judged not worth the fragility for this pass). The category
  create/persist/render/flyout path *was* verified end-to-end above, which exercises
  the same `ApplicationManager.Changed`/`CategoryManager.Changed` refresh plumbing
  the drop handler also depends on — but the drop handler itself is unverified by
  anything other than code review. Worth a manual click-test pass.

### Phase 4 — Running window detection and control
- `Native/User32.cs` extended with `EnumWindows`, `GetWindowText(Length)`,
  `GetWindowThreadProcessId`, `GetWindow`/`GetWindowLongPtr` (owner/tool-window
  filtering), `SetForegroundWindow`, `ShowWindowAsync`, `PostMessage` (graceful
  `WM_CLOSE`), and `SetWinEventHook`/`UnhookWinEvent`.
- `Native/DwmApi.cs` — `DwmGetWindowAttribute(DWMWA_CLOAKED)` to filter out
  DWM-cloaked windows (suspended UWP surfaces on another virtual desktop), which
  otherwise show up as ghost entries in naive `EnumWindows`-based taskbars.
- `Windows/WindowManager.cs` — enumerates real top-level windows with the same
  candidate filter real taskbars use (visible, unowned, not a bare tool window, not
  cloaked), plus Activate/Minimize/Maximize/Restore/Close. FlexTaskbar's own windows
  are excluded by process id, not by trying to special-case its own HWNDs.
- `Windows/WindowWatcher.cs` — event-driven via `SetWinEventHook` (Section 13
  explicitly prefers hooks over polling): `EVENT_SYSTEM_FOREGROUND` for activation,
  `EVENT_SYSTEM_MINIMIZESTART/END` for minimize state, `EVENT_OBJECT_CREATE/DESTROY/
  SHOW/HIDE` for open/close. `WINEVENT_SKIPOWNPROCESS` means FlexTaskbar's own
  windows never even generate events, for free.
- `TaskbarWindow`'s center panel now shows real running windows, grouped by owning
  executable (Section 12's "Chrome → 3 windows" example) — a single window gets its
  own button; 2+ windows collapse into one `"chrome (4)"`-style button that opens a
  per-window activation menu on click. Right-click on a single-window button gives
  Restore/Minimize/Maximize/Close.
- **Verified on the actual dev machine against real desktop state** (not synthetic
  data): enumeration correctly found and grouped the user's actual open windows
  (4 Chrome windows correctly collapsed into one "chrome (4)" button; Firefox, File
  Explorer, and other apps shown individually). Clicked into the Chrome group's menu,
  activated one specific window, and confirmed via `GetForegroundWindow()` that it
  genuinely became the OS foreground window — not just a UI state change on
  FlexTaskbar's side.
- This pass caught a real accessibility bug: composing button `Content` from an
  `Image`+`TextBlock` `StackPanel` (for the window icon) meant WPF couldn't
  auto-derive an `AutomationProperties.Name` from it, so every running-window button
  reported an empty accessible name (Section 39 requires accessible names — "do not
  rely exclusively on icons"). Fixed by setting `AutomationProperties.SetName`
  explicitly on both single-window and grouped buttons.

### Phase 5 — Start launcher, search, hotkeys
- `Native/User32.cs` extended with `RegisterHotKey`/`UnregisterHotKey`/`WM_HOTKEY`
  and `LockWorkStation`.
- `Services/GlobalHotkeyService.cs` — registers Win+Space via the taskbar window's
  own HWND message loop (no separate message-only window needed). Returns
  `false`/doesn't throw if the combination is already claimed — see verification below.
- `Services/PowerActionService.cs` — Lock (native `LockWorkStation`, no privilege
  needed), Sign out/Restart/Shutdown via the standalone `shutdown.exe` invoked with
  an explicit `ArgumentList` (never `cmd.exe`/`powershell.exe`, never a concatenated
  command string — Section 40). Sleep is not wired up (no `shutdown.exe` equivalent;
  would need `SetSuspendState` from `powrprof.dll` — left as a documented TODO).
- `Applications/ApplicationSearch.cs` — matches app name, executable file name, *and*
  category name (Section 51: searching "AI" returns everything filed under an "AI"
  category, including subcategories, not just an app literally named "AI"). Prefix
  matches sort before substring matches. Pure logic, no I/O — covered by 6 new unit tests.
- `ApplicationManager` now tracks recently-launched apps (`recents.json`, capped at
  10, newest first) — local-only, never transmitted (Section 37/38).
- `Views/LauncherWindow` — the Start button and the hotkey open the *same* window:
  search-first (live filter as you type), falling back to Recent / Categories / All
  Applications sections when the search box is empty. Positioned just above the
  taskbar, left-aligned — closes on Escape, on launching something, or on losing
  focus (click-away), matching real Start menu behavior. Settings is a visibly
  disabled placeholder with an explanatory tooltip (Phase 7 isn't built yet) rather
  than a dead-looking live button — Power is fully real.
- **Verified on the actual dev machine, including two bugs this specifically caught:**
  - Clicked Start → confirmed the launcher opens positioned correctly above the
    taskbar, with a working search box.
  - Typed `"code"` into search → correctly matched both "OpenCode" and "Visual
    Studio Code" (substring match) out of all 200 real installed apps.
  - Opened the Power menu and confirmed all 4 real items (Lock/Sign out/Restart/Shut
    down) are present — **deliberately did not click any of them**, since Restart/
    Shutdown/Lock would have disrupted the actual dev machine's session; correctness
    here rests on code review + the fact `shutdown.exe`/`LockWorkStation` are
    well-established, simple system calls, not on a live click-test.
  - Simulated a physical Win+Space keypress (`keybd_event`) — the hotkey did **not**
    fire; the launcher did not open. Confirmed via a diagnostic message that
    `RegisterHotKey` genuinely returned false, matching the documented expectation
    that Windows' own input-language-switch shortcut usually wins that race. Not a
    bug — the Start button remains the reliable way to open the launcher, exactly as
    designed for this case.
  - **Caught a real crash bug**: closing the launcher (via `WindowPattern.Close()`,
    simulating a user closing it) threw `InvalidOperationException` — `Deactivated`
    fired *during* `Window.Close()`'s own internal close sequence and re-entered
    `Close()`, which WPF forbids. Because **no app-wide unhandled-exception handler
    existed**, this single exception crashed the entire FlexTaskbar process,
    including the always-should-be-there taskbar window itself — a direct violation
    of spec Section 34/47 ("must not crash", "must not enter destructive states from
    a single bad callback"). Fixed two ways: (1) guarded the `Deactivated` handler
    with an `_isClosing` flag so it can't re-enter `Close()`; (2) added
    `Application.DispatcherUnhandledException` in `App.xaml.cs` as a proper safety
    net so *any* future UI-thread exception like this gets caught and logged instead
    of taking the whole taskbar down. Re-ran the exact same close sequence
    afterward and confirmed: launcher closes cleanly, taskbar and process both
    survive.
  - **Caught a second accessibility bug** (same root cause as Phase 4's): every
    `ListBoxItem` in the "All Applications"/search-results lists reported
    `"FlexTaskbar.Applications.ApplicationEntry"` (the raw type name) as its
    accessible name instead of the app name, because WPF falls back to `ToString()`
    when it can't derive a name from a composite `DataTemplate`. Fixed by overriding
    `ApplicationEntry.ToString()` to return `Name` — a one-line fix that retroactively
    corrects every list that binds `ApplicationEntry` directly, including the
    Phase 2/3 "All"/category popups, not just the new launcher.

### Phase 6 — Multi-monitor, auto-hide, AppBar work-area
- `Native/AppBar.cs` — `SHAppBarMessage` and `APPBARDATA`, in its own file rather than
  folded into `Shell32.cs`/`User32.cs` since it's a distinct, self-contained protocol.
- `Native/NativeTypes.cs` — shared `RECT` struct (used by both `AppBar.cs` and
  `User32.cs`'s monitor-info addition).
- `Native/User32.cs` extended with `EnumDisplayMonitors`/`GetMonitorInfo`.
- `Services/MonitorService.cs` — read-only monitor enumeration (bounds, work area,
  primary flag). Safe to call anytime; unlike AppBar registration it never changes
  anything on screen. Covered by 3 unit tests that run against the real display
  hardware (no mocking).
- `Taskbar/TaskbarLayoutManager.cs` — registers `TaskbarWindow` as a real Windows
  AppBar (`ABM_NEW` → `ABM_QUERYPOS`/`ABM_SETPOS` → `ABM_REMOVE`), which reserves
  actual screen work area the way the real Windows taskbar does. **Disabled by
  default** — this is the first Phase 6 feature that changes *shared* desktop state
  (every other window's maximize bounds, Win+Arrow snapping) rather than just
  FlexTaskbar's own window, so it stays off unless explicitly enabled, matching
  Section 2's safety-by-default requirement.
- Auto-hide: `TaskbarWindow` collapses to a 4px "hot edge" strip on `MouseLeave`
  (after a 500ms delay, to avoid flicker) and expands on `MouseEnter` — both real
  WPF window events, no polling loop or global mouse hook needed, since the
  collapsed strip is still a real window at the screen edge that receives them.
  Off by default.
- Both auto-hide and AppBar reservation are reachable via a right-click menu on the
  taskbar background (two checkable `MenuItem`s) — there's no Settings UI yet
  (Phase 7), and per "No Fake Features" these needed *some* honest, working entry
  point now that the underlying mechanisms are real.
- **Live-verified with the user's explicit permission, since this specifically
  changes shared desktop state**: registered the AppBar on the actual dev machine,
  independently confirmed (via a *separate*, non-FlexTaskbar `SPI_GETWORKAREA` call)
  that the real system work area genuinely shrank by the reserved amount, then
  gracefully closed the app and confirmed the work area was fully restored
  afterward. Only graceful shutdown was used for this specific test (never a forced
  kill), since a forced kill would skip the `Unregister()` cleanup path and could
  leave the reservation stuck until Explorer restarts.
- **This live test caught two real, launch-blocking bugs**, both now fixed:
  1. **DPI unit mismatch.** `GetMonitorInfo`/`SHAppBarMessage` both work in physical
     pixels; `Window.Left/Top/Width/Height` are in DIPs. The original code wrote the
     AppBar's physical-pixel rect straight into those DIP properties. On a
     non-100%-scaled display this corrupts the numbers outright — caught via a crash
     log showing `ArgumentException: '-8' is not a valid value for property
     'Height'`. Fixed by converting through `VisualTreeHelper.GetDpi(_window)` in
     both `ApplyPosition()` (the returned rect) and `ComputeRequestedRect()`
     (`BarThickness`, which is itself in DIPs).
  2. **Wrong anchor edge** (the one that actually caused the `-8`, once DPI was
     ruled out via added diagnostics): the code requested to dock at
     `monitor.Bounds.Bottom` — the literal physical screen edge — which is exactly
     where the *real* Windows taskbar already lives. The shell's `ABM_QUERYPOS`
     negotiation shrank the bottom edge to avoid the overlap but left the top edge
     alone, producing an inverted rect (`Bottom < Top`). Fixed by anchoring to
     `monitor.WorkArea.Bottom` instead — the edge of whatever space is *currently*
     free, i.e. just above the real taskbar. This is also the behaviorally correct
     choice: it keeps AppBar mode consistent with the "sits above the real taskbar"
     positioning every earlier phase already uses.
  - Debugging method: added temporary `Console.Error.WriteLine` diagnostics at each
    step (requested rect → post-QUERYPOS → post-SETPOS → DPI scale) and a temporary
    `--debug-appbar` CLI flag to trigger registration without needing mouse input
    (see below) — both removed after the bug was found and fixed, not left in the
    shipped code.
- **A real environment limitation surfaced during this phase, unrelated to
  FlexTaskbar's own code**: raw mouse-click injection (`mouse_event`/`SendInput` at
  verified-correct screen coordinates) does not reliably reach FlexTaskbar's windows
  in this dev/test environment, while UI Automation's `Invoke()`/`WindowPattern.Close()`
  and raw keyboard injection (`keybd_event`) both work reliably. This meant the
  right-click "Auto-hide"/"Reserve screen space" menu — which can only be opened via
  a real mouse right-click — could not be exercised through its actual UI entry
  point. Worked around for the AppBar test specifically by adding a temporary CLI
  flag to call the same code path directly; the menu-building code itself is the
  same well-established `ContextMenu`/`MenuItem` pattern already verified working
  multiple times in earlier phases (category flyouts, window context menus, the
  Power menu), so confidence is reasonably high, but the literal right-click gesture
  remains unverified by automation. A real click-test on a normal desktop session
  (outside this dev environment) would close that gap.
- Multi-monitor scope for this pass: monitor *enumeration* is real and tested
  against actual hardware, but the taskbar itself still only targets the primary
  monitor — no per-monitor taskbar replication, and running-window
  grouping/filtering doesn't take monitor placement into account. Section 19's
  "Primary only / All monitors / Per-monitor" setting options are not implemented;
  primary-only is simply the only behavior that exists right now.

### Phase 7 — System tray, Settings UI, startup, import/export
- `Tray/TrayIconService.cs` — real system tray icon via `System.Windows.Forms.NotifyIcon`
  rather than a hand-rolled `Shell_NotifyIcon` P/Invoke wrapper. Deliberate choice,
  documented in the csproj and here: NotifyIcon already correctly handles icon GDI
  handle lifetime, the hidden message-only window tray callbacks need, and
  Explorer-restart re-registration — reimplementing that by hand would be net
  negative for correctness, not "more native." Right-click menu: Settings, Rescan
  Applications, Exit; double-click opens Settings.
- **Combining `UseWPF` and `UseWindowsForms` in one csproj broke the whole project's
  compilation**, not just the tray file: both SDKs' implicit-usings machinery
  global-`using`s their own namespace (`System.Windows` and `System.Windows.Forms`),
  and the two collide on every shared type name (`Application`, `Button`, `Point`,
  `MouseEventArgs`, `KeyEventArgs`, `DragEventArgs`, `ListBox`, ...) — 12 `CS0104`
  ambiguous-reference errors across files that have nothing to do with the tray icon.
  Fixed by `<Using Remove="System.Windows.Forms" />` and `<Using Remove="System.Drawing" />`
  in the csproj, keeping WinForms types reachable only via an explicit `using` in
  `TrayIconService.cs` where they're actually needed.
- `Services/StartupService.cs` — per-user auto-start via a Startup-folder `.lnk`
  (Section 31), not a registry Run key — no admin privilege needed, and it's exactly
  what Task Manager's own Startup tab already understands. Reuses the same
  `IShellLinkW`/`IPersistFile` COM pattern from Phase 2's shortcut *reading*, just
  writing instead (`SetPath`/`SetWorkingDirectory` + `IPersistFile.Save`).
- `Settings/SettingsWindow.xaml(.cs)` — the real home for auto-hide and AppBar
  reservation, which Phases 5–6 had stuck on a temporary right-click menu (that menu
  now also has a "Settings..." entry, and both entry points share the same
  persisting methods on `TaskbarWindow` so they can't drift out of sync). Tabs:
  General (start-with-Windows, auto-hide, reserve-screen-space, rescan), Appearance
  (position, height), Keyboard (hotkey preset), Advanced (open config folder,
  export/import, reset). No separate "Categories" tab — category management already
  has a real home (each category's own "⚙ Manage Category" flyout item), so Settings
  doesn't duplicate it with a second, competing tree editor.
- `Settings/AppSettings.cs` + `SettingsManager.cs` — `settings.json` via the existing
  `ConfigurationService`, loaded at `TaskbarWindow` construction time (before initial
  positioning, so a saved position/height/AppBar preference takes effect from the
  very first frame, not after a visible jump).
- `GlobalHotkeyService.Register(modifiers, key)` generalizes what Phase 5 hardcoded
  to Win+Space; `Settings/HotkeyPresets.cs` is a small curated lookup (Win+Space,
  Ctrl+Alt+Space, Ctrl+Shift+Space, Ctrl+Alt+K) rather than a full "press any key"
  capture control — closes the "hotkey isn't configurable" gap flagged after Phase 5
  confirmed Win+Space genuinely loses its OS-level claim on this dev machine.
- Import/export (Section 41): `Settings/BackupData.cs` bundles categories, app
  category/pin/favorite assignments (`Applications/ApplicationAssignment.cs` — the
  *customization* subset, not the full regenerable app list), and settings into one
  JSON file via standard `SaveFileDialog`/`OpenFileDialog`. Reset Configuration
  timestamp-backs-up `categories.json`/`settings.json`/`recents.json` before clearing
  them (`applications.json` is left alone — it's just a cache, rescanning
  regenerates it regardless).
- **Verified live, extensively, via UI Automation against the real running app**
  (not just build success): toggled "Start with Windows" and independently confirmed
  the `.lnk` was actually created in — and later removed from — the real Startup
  folder on disk; switched the hotkey preset to Ctrl+Alt+K via real keyboard
  navigation (the ComboBoxItem itself didn't expose a usable `SelectionItemPattern`,
  worked around with actual arrow-key/Enter input) and confirmed both the "registered
  successfully" status message and the persisted `settings.json` value; dragged the
  Appearance height slider and confirmed the *actual taskbar window* resized live
  (40px → 56px, bottom edge staying anchored); switched Position to Top and confirmed
  the window really moved to `Top=0`; then restored both to defaults and confirmed
  the window returned to its original bounds exactly.
- **This pass caught two more real bugs**, both fixed:
  1. A `NullReferenceException` in the Appearance tab's height-slider handler: XAML's
     `Minimum`/`Maximum` attributes coerce the `Slider`'s `Value` and fire
     `ValueChanged` *during* `InitializeComponent()`, before `HeightValueText`
     (declared later in the same `StackPanel`) had been connected yet. The existing
     `_isInitializing` guard didn't help because the code touched the null field
     *before* checking that flag. Fixed by null-checking the field first. Notably,
     Phase 5's `DispatcherUnhandledException` safety net caught this live and kept
     the whole app running rather than crashing it — proof that safety net earns its
     keep, not just theory.
  2. The 12-file `CS0104` ambiguity storm described above from combining
     `UseWPF`/`UseWindowsForms` — a real, blocking build failure, not a style nit.

### Phase 8 — Safe mode, recovery, Explorer-restart tolerance, crash-loop protection
- `--safe-mode` is now real (Section 30): `App.xaml.cs` never constructs
  `TaskbarWindow` at all in this path — it shows only `Views/RecoveryWindow`
  (`isSafeMode: true`), so there's no AppBar registration, no hotkeys, no auto-hide,
  nothing that could compound whatever's broken. Previously this flag just exited.
- `Views/RecoveryWindow.xaml(.cs)` — the Section 30 recovery menu (Restore Windows
  Taskbar / Restart Explorer / Disable Auto-Start / Open Settings / Restart in Safe
  Mode), reachable via `Ctrl+Alt+Shift+F12` from a normal running instance *or* as
  the sole window in Safe Mode. Deliberately built to need no live `TaskbarWindow` —
  every action operates on persisted config/OS state directly, and only
  *additionally* touches a live instance when one is passed in. This is what "must
  work even if the normal taskbar is unavailable" (Section 30) actually requires
  architecturally, not just states as a goal — confirmed by testing it standalone
  under `--safe-mode` with no taskbar running at all.
- `Services/InstanceSignalingService.cs` — `--restart`/`--reset` now really restart
  the running instance (Section 54) instead of just bouncing off the single-instance
  mutex like a normal second launch. Built on a named `EventWaitHandle`: the new
  process signals it, the running instance's `ThreadPool.RegisterWaitForSingleObject`
  callback marshals a graceful `Close()` back onto its own UI thread, and the new
  process waits on the mutex (with a timeout) to confirm the old one actually let go
  before proceeding.
- `Services/ResetService.cs` — the reset logic Settings' "Reset Configuration"
  already had, extracted so `--reset` (CLI) and the Settings button are one
  implementation, not two that could drift.
- `Services/CrashGuardService.cs` — crash-loop protection (Section 47) via a
  dirty-flag: every startup marks state "unclean" and only a graceful `OnExit`
  clears it, so a new startup that finds the flag still dirty knows the previous run
  never reached a clean exit. Three consecutive unclean exits disables Startup
  auto-start (`StartupService.SetEnabled(false)`) — the app still launches that one
  time rather than refusing to start, so the user isn't locked out.
- `AppDomain.CurrentDomain.UnhandledException` added alongside the existing
  `DispatcherUnhandledException` (Phase 5) — covers fatal exceptions on non-UI
  threads, which the Dispatcher handler can't catch. Can't prevent termination for
  those (nothing can), but at least logs before the process dies instead of vanishing.
- `TaskbarLayoutManager.HandleShellRestarted()` + `TaskbarWindow`'s
  `RegisterWindowMessage("TaskbarCreated")` hook — Explorer-restart tolerance for the
  AppBar reservation (Section 46): Explorer owns the registered-AppBar list, so its
  restart silently drops our reservation unless we re-claim it.
- **Extensively live-verified, including a repeat of the "only ask before,
  test carefully" pattern from Phase 6** for the two actions that touch shared
  system state:
  - `--safe-mode`: launched it standalone and confirmed via UI Automation that
    *only* the Recovery window exists (no taskbar window at all), "Open Settings" is
    correctly disabled with an explanatory tooltip, and clicking "Restore Windows
    Taskbar" genuinely wrote `settings.json` — proving recovery really works with
    zero live taskbar state to lean on.
  - The `Ctrl+Alt+Shift+F12` hotkey: triggered via real physical key injection
    (not UI Automation `Invoke` — an actual `RegisterHotKey`/`WM_HOTKEY` round trip),
    opened Recovery with a live `TaskbarWindow` reference this time, and confirmed
    "Open Settings" was now correctly *enabled*.
  - `--restart`: launched a second process with `--restart` while a normal instance
    was running; confirmed via process ID that the *first* instance exited
    gracefully and the *second* took over as the one and only taskbar window —
    not two competing instances, not zero.
  - `--reset`: created a real test category, ran `--reset`, and confirmed
    `categories.json` came back empty *and* a timestamped `.resetbackup` copy
    containing the original category existed — the backup-before-reset contract
    holds under the CLI path, not just the Settings button.
  - Crash-loop protection: simulated three consecutive forced-kill "crashes" via
    real process launches (not a unit test with mocked state), then confirmed the
    4th launch logged the auto-disable message with the exact predicted count.
    Confirmed a single graceful close resets the streak to zero.
  - **AppBar reservation across a real Explorer restart — with the user's explicit
    permission, since this is more disruptive than anything tested before it**:
    enabled screen-space reservation, independently confirmed the real system work
    area had shrunk, then actually killed and relaunched `explorer.exe`. **The
    reservation was lost and not automatically re-claimed** — a genuine, confirmed
    gap in the initial `WM_TASKBARCREATED`-only implementation (the broadcast
    message did not reliably trigger the handler in this environment, consistent
    with other message-delivery quirks already documented in Phase 6's testing
    notes). Responded by adding a second, independent mechanism rather than just
    reporting the bug: a 20-second `DispatcherTimer` that unconditionally calls
    `HandleShellRestarted()` (full `ABM_NEW` re-registration, not just
    `ApplyPosition()` — a plain reposition can't recover a registration Explorer has
    forgotten about entirely) whenever a reservation is supposed to be active. A
    follow-up end-to-end re-test (second Explorer restart) was attempted but the
    test session state became ambiguous (mismatched coordinate spaces across
    repeated UI Automation queries — see the note on physical-pixel vs.
    automation-reported coordinates below) before a clean confirmation could be
    captured; rather than risk a third live Explorer restart chasing a clean
    reading, the session was intentionally ended by gracefully closing and
    confirming the desktop returned to its normal, unreserved state. **The fix is
    implemented and reasoned through carefully, but the specific "timer catches what
    the message missed" recovery path has not been independently re-confirmed by a
    clean live test** — worth a dedicated verification pass before relying on it.
- **Environment note, not a FlexTaskbar bug**: `System.Windows.Forms.SystemInformation.
  WorkingArea` and raw `SystemParametersInfo(SPI_GETWORKAREA)` consistently reported
  physical-pixel values (e.g. `3840×2160` panel), while some UI-Automation-reported
  `BoundingRectangle` values earlier in this session read closer to `1920×1080`. The
  likely explanation: the PowerShell process driving UI Automation isn't itself
  manifest-declared DPI-aware, so Windows may virtualize/scale some values it
  reports back to it, while raw Win32 calls like `SPI_GETWORKAREA` are never
  virtualized. This reconciles what looked like conflicting DPI-scale evidence
  across earlier phases — it was a quirk of the *test* tooling's own DPI awareness,
  not of FlexTaskbar's actual behavior (which was independently confirmed correct
  via the non-virtualized raw calls throughout).

### Phase 9 — Installer, documentation
- `installer/FlexTaskbar.Installer.wixproj` + `Product.wxs` — a real, buildable,
  **verified-working** per-user WiX MSI installer. `dotnet build FlexTaskbar.sln`
  now builds the app, tests, *and* installer together via a `ProjectReference` from
  the installer to the app project, so the installer always packages whatever the
  app project actually built (no hand-maintained file list to go stale).
- Three independently-selectable features via `WixUI_FeatureTree`: the app itself +
  Start Menu shortcut (required), a desktop shortcut (optional, on by default), and
  a Startup-folder shortcut for "start with Windows" (optional, **off by default** —
  Level 1000 in WiX, matching the same safety-by-default posture as every runtime
  toggle in the app itself). The Startup shortcut this installs is the exact same
  file `Services/StartupService.cs` manages at runtime, so the installer checkbox
  and the in-app Settings toggle can never disagree with each other.
- **Two real build/packaging obstacles hit and resolved, not just "it built"**:
  1. WiX Toolset v7 requires accepting a paid "Open Source Maintenance Fee" EULA to
     use — this is a licensing/business decision, not something to silently agree
     to on the user's behalf. Used WiX v6 (the latest version without that
     requirement) instead.
  2. Per-user MSI packages trigger `ICE38`/`ICE64`/`ICE91` validation
     errors/warnings by default — legacy Windows Installer consistency checks that
     assume every user-profile file needs an HKCU-registry keypath and explicit
     `RemoveFile` tracking, predating per-user installs being a common,
     well-supported pattern. Suppressed validation (`SuppressValidation=true`) —
     this doesn't change what actually gets installed or how uninstall behaves, it
     just skips an overly conservative advisory check, and was confirmed safe by
     the live install/uninstall test below.
- **Live-verified, not just "the MSI built"**: ran the actual built `.msi` through
  `msiexec /i ... /qn` and confirmed on disk — install directory populated with all
  runtime files (`.exe`, `.dll`, `.deps.json`, `.runtimeconfig.json`, and the
  WinForms/WinRT dependencies the tray icon needs), Start Menu shortcut present,
  Desktop shortcut present (default-on feature), Startup shortcut correctly
  **absent** (default-off feature, confirming the opt-in logic works, not just that
  files got copied). Then ran `msiexec /x` and confirmed clean removal of all three
  — and confirmed `%APPDATA%\FlexTaskbar` (user settings) was untouched by
  uninstall, exactly as designed (Section 44's "never silently delete settings").
- **One confirmed gap from that same live test**: the installed product does not
  appear in Windows' "Programs and Features"/Add-or-Remove-Programs list, even
  though the underlying MSI `RegisterProduct`/`PublishProduct` standard actions
  both completed successfully (confirmed by inspecting the verbose install log) and
  `msiexec /x` uninstalls correctly by product path regardless. This is specifically
  about the Control Panel *display* entry being missing, not a functional
  installer defect — documented honestly in
  [INSTALLATION.md](../docs/INSTALLATION.md) and
  [TROUBLESHOOTING.md](../docs/TROUBLESHOOTING.md) rather than glossed over.
- Documentation pass: [INSTALLATION.md](../docs/INSTALLATION.md) rewritten with
  real install/uninstall steps (replacing Phase 1's "no installer yet" stub),
  [ARCHITECTURE.md](../docs/ARCHITECTURE.md) rewritten to describe the actual final
  structure across all 9 phases (replacing Phase 1's initial sketch),
  [TROUBLESHOOTING.md](../docs/TROUBLESHOOTING.md) expanded with real issues found
  during Phases 2–9 (the `CS0104` WinForms/WPF ambiguity, the XAML-construction-time
  null-reference pattern, the AppBar/DPI trap, etc.) rather than only Phase 1's
  generic build troubleshooting, [RECOVERY.md](../docs/RECOVERY.md) rewritten to
  describe what Phase 8 actually built (it previously described a "planned"
  mechanism), and a new [MANUAL_TESTING.md](../docs/MANUAL_TESTING.md) documenting
  the spec's test matrix (Section 45) — explicitly separating what was verified
  during development on this one machine/configuration from what still needs a
  dedicated pass (Windows 10, multi-monitor, non-100% DPI scaling, etc.) before a
  real release.

## Known issues / deliberate gaps (not bugs — honestly scoped out across all 9 phases)

- No pinned-app area on the taskbar yet — `ApplicationEntry.IsPinned` is modeled
  and persisted but not surfaced in any UI (no way to set or view it). Reasonable
  next small addition, not done in this pass.
- No multi-monitor taskbar replication — the taskbar itself still only targets the
  primary monitor (enumeration is real; using it for anything beyond the primary
  monitor is not). Window enumeration doesn't filter/group by monitor either.
- Auto-hide's interaction with a *reserved* (AppBar) taskbar was only verified at
  the code-path level (both call the same `ApplyPosition`), not click-tested live —
  physical mouse-hover simulation isn't reliable in this dev environment.
- Measured memory: ~130 MB (Debug), ~113 MB (Release) working set for the Phase 1
  empty shell — already above the <100 MB target (Section 27), essentially all
  WPF/CLR baseline load cost, not FlexTaskbar-specific. Still not re-measured with
  everything from Phases 2–8 loaded at once — genuinely open.
- Icons are extracted **synchronously on the UI thread** the first time each list/menu
  item is realized. Deferred rather than adding async-loading complexity.
- Second-instance focus-existing-window behavior is *partially* addressed: a plain
  second launch still exits silently (no focus-and-activate), but `--restart` and
  `--reset` now correctly hand off to a fresh instance. A plain second launch
  focusing the existing window instead of exiting is still a TODO.
- Sleep is not implemented in the Power menu (no `shutdown.exe` equivalent; would
  need `SetSuspendState` from `powrprof.dll`).
- `DispatcherUnhandledException`/`AppDomain.UnhandledException` still only write to
  `Console.Error` — real structured logging under `%LOCALAPPDATA%\FlexTaskbar\Logs\`
  (Section 33) is still not implemented. These are safety nets against total
  crashes, not a logging system, and three phases' worth of real saves (Phase 5, 7,
  and implicitly every crash-guard-tracked exit) make a stronger case each time that
  it's worth finishing properly.
- `.url` shortcuts are discovered and validated (http/https-only) but don't get an
  icon yet (no generic "web app" placeholder glyph).
- No filesystem-watcher-based rescan-on-change (Section 7) — rescans happen at
  startup or via the manual "Rescan Now" button (Settings/tray menu); no periodic
  auto-rescan.
- **Category-to-category dragging and in-category app reordering are not
  implemented** — Section 9 also asks for these; Phase 3 only implements
  app-to-category drag (from the "All" list) plus the full rename/add-sub/delete CRUD.
- No "Move to Category" right-click item on individual apps yet (Section 50) —
  the only way to (re)categorize an app is dragging it from "All" onto a *root*
  category button. `CategoryManager.GetAllFlattened()` already exists for exactly
  the picker this would need — reasonable next small addition.
- No custom category icon picker — `IconGlyph` defaults to 📁 for every new category.
- Drag/drop's actual mouse-driven path (app → category) is unverified by automated
  testing — synthetic UI Automation can't drive real native drag gestures. Code
  follows the standard WPF pattern and has been reviewed, not click-tested.
- Running-window buttons rebuild the *entire* center panel on every foreground/open/
  close/minimize event rather than diffing incrementally. Simple and correct; cheap
  enough at real-world window counts (verified: no visible lag with ~10 windows open
  during testing), but a diff-based update would be the natural next optimization if
  it ever shows up as a real cost.
- No live title updates — `EVENT_OBJECT_NAMECHANGE` isn't hooked, so a window whose
  title changes after being tracked (e.g. a browser tab switch) won't update its
  button label until the window is closed/reopened or another tracked event fires
  for it incidentally. Deliberately deferred rather than adding a 4th hook range for
  a cosmetic gap.
- Grouped-window buttons (2+ windows of the same app) only offer "click a title to
  activate it" — no right-click Restore/Minimize/Maximize/Close at the group level
  (only single-window buttons get the full context menu). A per-window context menu
  inside the group's activation menu would close this gap.
- Settings' Reset Configuration is a single "reset everything" action, not the
  granular per-section resets (appearance/categories/applications/full) Section 42
  describes — a reasonable scope cut given the atomic-backup mechanics are shared
  regardless of granularity.
- Export/import round-trips categories, app assignments, and settings correctly
  (unit-tested), but the actual `SaveFileDialog`/`OpenFileDialog` interaction
  (native Win32 common dialogs) wasn't exercised by live UI automation — modal
  common dialogs are a known-hard target for the same input-injection reasons
  documented in Phase 6. The underlying read/write logic was verified via other
  paths that share it (e.g. `CategoryManager.ReplaceAll`, `ApplicationManager.
  ApplyAssignments` are both exercised indirectly, and `BackupData`'s JSON shape is
  unit-tested), but the file-picker UI itself is not.
- **The AppBar Explorer-restart recovery fix (the 20-second `HandleShellRestarted()`
  polling timer) has not been independently re-confirmed by a clean live test** —
  see the Phase 8 changelog above for exactly what was and wasn't verified. This is
  the single most important thing to verify before relying on AppBar reservation
  mode in any real-world Explorer-restart scenario.
- "Replace Windows Taskbar" mode in the full sense the spec describes (actually
  hiding/superseding the real taskbar, not just reserving space alongside it) is
  still not implemented — Phase 6 built the AppBar mechanism and Phase 8 built the
  recovery/safety net around it, but nothing yet makes FlexTaskbar the *primary*
  taskbar. Given Section 2's overriding safety requirement ("first version must run
  safely alongside the existing taskbar"), this is arguably appropriately out of
  scope for now rather than a gap — genuine taskbar replacement is a significant
  further step that deserves its own dedicated, carefully-scoped pass with the
  user's explicit sign-off before attempting, not something to fold into a phase
  named for its safety net.
- Focus-stealing on recovery: `RecoveryWindow` is `Topmost="True"`, which is
  appropriate for an emergency-access window, but hasn't been tested for how it
  interacts with other topmost windows (e.g. `TaskbarWindow` itself, also topmost).
- **Installer doesn't appear in Programs and Features / Add-or-Remove Programs**,
  despite installing/uninstalling correctly via `msiexec` and the underlying MSI
  product registration succeeding — see the Phase 9 changelog above and
  [INSTALLATION.md](../docs/INSTALLATION.md) for the full story. Worth investigating
  before a real release; not blocking for a `msiexec`-driven install/uninstall workflow.
- No code-signing on either `FlexTaskbar.exe` or the installer — both will trigger
  Windows SmartScreen warnings on first run for anyone who downloads them, which is
  normal for an unsigned indie tool but worth knowing about.
- `docs/MANUAL_TESTING.md` is new this phase and documents its own gaps in detail —
  see it directly rather than duplicating that list here.

## Suggested follow-ups (no phase currently planned — see docs/MANUAL_TESTING.md for the test-coverage half of this list)

There is no Phase 10 on the books. If picking this project back up, the highest-value
next steps, roughly in order of "closes a real gap noted above":

1. Re-confirm the Phase 8 AppBar/Explorer-restart fix with a clean, isolated live
   test (the most safety-relevant unverified item in the whole project).
2. Investigate the installer's missing ARP display entry.
3. Structured logging under `%LOCALAPPDATA%\FlexTaskbar\Logs\` (Section 33) — three
   separate phases' worth of real crash/diagnostic saves have made the case that the
   current `Console.Error`-only safety net should graduate into an actual logging system.
4. A pinned-apps UI surfacing `ApplicationEntry.IsPinned`, which has been modeled and
   persisted since Phase 2 but never given anywhere to be set or viewed.
5. Cross-machine/cross-configuration testing per `docs/MANUAL_TESTING.md` — Windows
   10, multi-monitor, non-100% DPI scaling, dark/light system theme — all currently
   untested beyond this one dev machine.

## Architecture notes

- `Native/` centralizes P/Invoke: `Shell32.cs` (SHGetFileInfo, IShellLinkW/IPersistFile),
  `User32.cs` (icon/window-management/WinEventHook/hotkey/monitor-info surface),
  `DwmApi.cs` (cloak detection), `AppBar.cs` (SHAppBarMessage), `NativeTypes.cs`
  (shared `RECT` struct used by both `User32.cs` and `AppBar.cs`).
- **Physical pixels vs. DIPs is a recurring trap worth calling out explicitly**:
  `GetMonitorInfo`, `SHAppBarMessage`, and most raw Win32 coordinate APIs work in
  physical pixels; WPF's `Window.Left/Top/Width/Height` (and `SystemParameters.WorkArea`)
  are in DIPs (96-DPI logical units). `TaskbarLayoutManager` converts explicitly via
  `VisualTreeHelper.GetDpi(_window)` — any *new* code that mixes a raw Win32 rect
  with a WPF layout property needs the same conversion, or it'll work fine at 100%
  scaling and silently break (or crash, as this one did) at any other scale factor.
- `Windows/WindowManager` and `Windows/WindowWatcher` are deliberately separate:
  `WindowManager` is stateless (pure enumeration + control operations),
  `WindowWatcher` is the only stateful piece (owns the hook handles, implements
  `IDisposable`). `TaskbarWindow` owns the actual `Dictionary<IntPtr, WindowInfo>`
  state and reconciles watcher events into it — no other class holds "the" window list.
- `Services/ConfigurationService` (JSON under `%APPDATA%\FlexTaskbar\`, atomic writes +
  `.bak` restore) now backs five files unchanged: `applications.json`,
  `categories.json`, `recents.json`, and `settings.json` — the pattern held up
  without modification across all of them.
- `TaskbarWindow` exposes a handful of `internal` accessors/methods
  (`ApplicationManagerInstance`, `ApplyPositionAndHeight`, `ApplyHotkeyPreset`, etc.)
  specifically so `SettingsWindow` can drive real taskbar state without either class
  reaching into the other's private fields — same-assembly `internal` rather than a
  fully public API surface, since nothing outside FlexTaskbar.dll needs this.
- Combining `UseWPF` and `UseWindowsForms` in one csproj is workable but not free —
  see the Phase 7 changelog above for the `<Using Remove>` fix required. Any future
  addition that needs a WinForms type should add its own file-scoped
  `using System.Windows.Forms;`, not rely on it being globally available.
- `Utilities/AppPaths.cs` is the single source of truth for `%APPDATA%\FlexTaskbar\`
  vs. `%LOCALAPPDATA%\FlexTaskbar\IconCache\` — config vs. disposable cache, per
  Section 28's "never store config in the install directory" rule.
- `Menus/CategoryMenuBuilder` reads `ApplicationManager.Applications` and
  `CategoryManager.Categories` live at menu-build time (not a cached snapshot), so a
  category's flyout is always current without needing its own change-tracking.
  `Views/LauncherWindow` reuses the same `CategoryMenuBuilder` instance for its
  Categories section rather than duplicating flyout logic.
- `App.xaml.cs` now installs a `DispatcherUnhandledException` handler — any new
  window/dialog added in future phases inherits this safety net automatically; it
  does not need its own try/catch around every event handler, but re-entrancy bugs
  like the one caught in Phase 5 (calling `Close()` from `Deactivated` without a
  guard) are still worth avoiding at the source, since the handler is a safety net,
  not a substitute for correct code.
- `Views/RecoveryWindow` takes a *nullable* `TaskbarWindow?` deliberately — this is
  the one place in the codebase designed from the start to work with or without a
  live taskbar instance, since Section 30 requires recovery to function even when
  the taskbar itself is broken. Any future recovery-adjacent feature should follow
  the same pattern (operate on persisted state first, touch a live instance only as
  a bonus) rather than assuming `TaskbarWindow` exists.
- Two independent mechanisms now defend the AppBar reservation against Explorer
  restarting: a `WM_TASKBARCREATED`-triggered fast path
  (`TaskbarWindow.ShellMessageWndProc` → `TaskbarLayoutManager.HandleShellRestarted`)
  and a 20-second polling fallback (`_appBarHealthTimer`) calling the same method.
  This redundancy exists specifically *because* live testing showed the message-only
  approach wasn't reliable in this environment — don't remove the polling timer as
  "redundant" without re-confirming the message path is trustworthy first.
- `Services/CrashGuardService` and `Services/InstanceSignalingService` both use the
  same "named OS primitive keyed by a fixed GUID-suffixed string" pattern already
  established by the single-instance mutex (`App.xaml.cs`) and the hotkey services —
  consistent naming convention (`Local\FlexTaskbar.<Purpose>.9F3B2C7A` /
  `.RestartRequested.9F3B2C7A`) makes it easy to `grep` for every named OS object
  FlexTaskbar creates.
