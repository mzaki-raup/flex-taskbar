# FlexTaskbar roadmap

Ideas for what comes next, grouped by theme. Each has a difficulty and a
short note on what it involves. Tick an item when it is merged to `main`
and described in the README.

**Difficulty:** 🟢 Easy (a day or less) · 🟡 Medium (a few days) · 🔴 Hard
(a week or more, or needs real-Windows testing that Wine can't do)

## Recommended order

Done: dark settings windows, keyboard control of the flyouts, running-app
indicators, auto-hide, and settings backup and restore.

Suggested next:

1. **Closing animations** (🟢). Rounds off the opening animations already
   there.
2. **A hotkey per category** (🟢). Builds on the keyboard control.
3. **Theme presets** (🟢). Builds on the backup format for import and
   export.
4. **Pin folders and files to the bar** (🟡). The most-asked-for dock
   feature still missing.
5. **Measured performance** (🟡). Real numbers for the README.

## Everyday use

- [x] **Running-app indicators** · 🟡
  A dot under bar and flyout icons for apps that are open. Clicking a
  running app switches to its window instead of starting another copy
  (Shift+click starts a new one). Follows windows through the shell's
  window notifications (no polling) and matches them to apps by
  AppUserModelID or program path.
- [x] **Auto-hide bar** · 🟡
  The bar slides or fades out of view and comes back when the pointer
  reaches the screen edge, with a delay setting; it reserves no screen
  space while auto-hiding.
- [x] **Keyboard control of the flyouts** · 🟡
  Arrow keys move between tiles and rows, Enter launches or opens a
  subcategory, Esc goes back, Tab moves between the bar's flyouts, and
  typing jumps to an app. The bar hotkey (Ctrl+Alt+B) or `--bar` opens
  *All* ready for the keyboard.
- [ ] **Multi-monitor** · 🔴
  A bar on every screen, or on a chosen one, each docked and reserving
  space on its own monitor. Hard to test without real multi-monitor
  Windows.
- [ ] **Jump lists and quick actions** · 🔴
  Right-click an app for its recent files (Windows jump lists), *Open file
  location*, *Run as administrator* and *Uninstall*. Jump lists come
  through undocumented or COM-heavy APIs, so this needs care and real
  Windows.

## Organising

- [ ] **Pin folders and files to the bar** · 🟡
  A folder on the bar opens a flyout listing its contents (like Dock
  stacks), sorted by name or date; sub-folders open further flyouts.
- [ ] **Drag from a flyout or the All list** · 🟡
  Drag an app onto the bar to pin it, or onto a category to file it there.
- [ ] **Smart categories** · 🟡
  Filled automatically ("Recently installed", "Most used", "Package
  manager tools"), or by simple rules (kind is…, path contains…). Kept
  separate from hand-made categories so nothing is moved by surprise.
- [ ] **Bar profiles** · 🟡
  Several bars ("Work", "Gaming") with their own pinned apps and order,
  switched from the tray menu or a hotkey.
- [ ] **A hotkey per category** · 🟢
  Opens that category's flyout at the pointer, like the menu hotkey.

## Look

- [x] **Dark mode for the settings windows** · 🟢
  The bar, flyouts, menus and search window follow the theme, but the
  Manage, Appearance, *Arrange the bar* and custom-app windows are always
  light. Give `panel` a dark palette and apply dark title bars and control
  themes.
- [x] **Closing animations** · 🟢
  The opening styles (Fade, Slide, Scale, Drawer, Genie) played in
  reverse when a flyout closes, without delaying the next one opening.
- [ ] **Mica or acrylic behind the bar and flyouts** · 🔴
  Windows 11's blurred backdrop as an alternative to plain translucency.
  Layered windows can't use it directly, so this means a different window
  setup and careful testing on Windows 10 and 11.
- [ ] **Theme presets** · 🟢
  Save the current look under a name, switch between saved looks, and
  import or export a look as a small file (pictures included).

## Reliability and upkeep

- [x] **Settings backup and restore** · 🟢
  Export everything (`config.json` and the `icons` folder) as one zip, and
  import it on another PC, after checking the names in it the same way
  pictures are checked today.
- [ ] **Measured performance** · 🟡
  The README says memory and CPU use haven't been measured on Windows. Add
  a small diagnostics page (memory, redraw times, timers running) and
  publish real numbers.
- [ ] **Checks on real Windows** · 🔴
  What Wine couldn't verify (see the README): high-DPI scaling, global
  hotkeys, Store and web-app icons, dragging files in, the bar beside the
  real taskbar, *Start with Windows*. A UI-automation smoke test in CI
  would keep them working.
- [ ] **Accessibility** · 🟡
  Screen-reader names and roles for the bar and flyout buttons (UI
  Automation), visible keyboard focus, and a high-contrast theme that
  follows Windows' contrast setting.
