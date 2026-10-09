# FlexTaskbar roadmap

Ideas for what comes next, grouped by theme. Each has a difficulty and a
short note on what it involves. Tick an item when it is merged to `main`
and described in the README.

**Difficulty:** 🟢 Easy (a day or less) · 🟡 Medium (a few days) · 🔴 Hard
(a week or more, or needs real-Windows testing that Wine can't do)

## Recommended order

**Every item below is done.** The last four — multi-monitor, jump lists and
quick actions, a Windows 11 backdrop behind the bar, and the checks on real
Windows — went in together; everything before them was already in.

What is left is not a feature but a short list of things no one has been able
to *see* working yet, kept in the README under *Still not verified*: two or
more monitors (and mixed DPI between them), a Windows taskbar on the top or a
side, a real drag from Explorer, importing old .NET settings, and NVDA and
JAWS. Each needs hardware or a setup the machine this was built on doesn't
have, rather than more code.

The one piece of unmade work named below: the **flyouts** have no Windows 11
backdrop, because their opening animations need the layered window a backdrop
can't be. That trade is described under *Look*.

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
- [x] **Multi-monitor** · 🔴
  *Screens* in the Appearance window: the main screen, every screen, or one
  chosen screen. Each bar is docked and reserves space on its own monitor,
  at that monitor's DPI; attaching or detaching a screen adds or removes its
  bar and leaves the others alone. The choosing and add/remove logic is unit
  tested, and two screens at different scalings have now been seen working on
  a real PC: a bar on each, docked and reserving space on its own monitor, at
  its own DPI. Only attaching or detaching a screen while running is still
  unseen (see the README's *Still not verified*).
- [x] **Jump lists and quick actions** · 🔴
  Right-click an app, on the bar or in a flyout, for the files it was last
  used on (its Windows jump list, read through `IApplicationDocumentLists`)
  and for *Open file location*, *Run as administrator* and *Uninstall*. The
  actions are whatever the shell reports for that app, by canonical verb, so
  each app offers only what Windows can really do with it.

## Organising

- [x] **Pin folders and files to the bar** · 🟡
  A folder on the bar opens a flyout listing its contents (like Dock
  stacks), folders first then by name; sub-folders open further flyouts.
- [x] **Drag from a flyout or the All list** · 🟡
  Drag an app onto the bar to pin it, or onto a category to file it there.
- [x] **Smart categories** · 🟡
  Filled automatically ("Recently installed", "Most used", "Package
  manager tools"), or by simple rules (kind is…, path contains…). Kept
  separate from hand-made categories so nothing is moved by surprise.
- [x] **Bar profiles** · 🟡
  Several bars ("Work", "Gaming") with their own pinned apps and order,
  switched from the tray menu or a hotkey.
- [x] **A hotkey per category** · 🟢
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
- [x] **Mica or acrylic behind the bar** · 🔴
  *Backdrop* in the Appearance window: Mica, Acrylic or Mica Alt behind the
  bar on Windows 11 22H2 and later, greyed out where Windows can't draw one.
  A layered window can't have a backdrop, so with one chosen the bar is built
  as an ordinary window and painted into; the margin, *Fit to icons* and
  *Fade* auto-hide don't apply while it is on (see the README).
  **The flyouts still don't have one** — their opening animations need the
  layered window a backdrop can't be, so that trade is left unmade.
- [x] **Theme presets** · 🟢
  Save the current look under a name, switch between saved looks, and
  import or export a look as a small file (pictures included).

## Reliability and upkeep

- [x] **Settings backup and restore** · 🟢
  Export everything (`config.json` and the `icons` folder) as one zip, and
  import it on another PC, after checking the names in it the same way
  pictures are checked today.
- [x] **Measured performance** · 🟡
  *Diagnostics…* (tray menu, `--diagnostics`) reports memory, CPU time,
  GDI/USER objects, handles and bar and flyout drawing times; the README
  has numbers measured under Wine. Numbers from real Windows are still to
  come (see *Checks on real Windows*).
- [x] **Checks on real Windows** · 🔴
  Everything Wine couldn't verify has now been seen on Windows 11 25H2 —
  high-DPI scaling, Store and web-app icons, dropping files in, the bar
  beside the real taskbar, *Start with Windows*, Narrator, and high contrast
  — and `tools\smoke-test.ps1` keeps the core of it working: CI starts the
  exe it just built and checks, through Windows itself, that the bar appears,
  docks, reserves its screen space and gives it back, and that a screen
  reader reads it as a toolbar of named, placed, pressable buttons.

  Two notes. The bar answers **MSAA**, not UI Automation — a UIA client sees
  no children — so the test asks the way a screen reader does. And the smoke
  test needs a desktop: a runner started as a Windows *service* has none, so
  it skips there rather than failing. What is still unseen (two monitors, a
  taskbar on another edge, a real Explorer drag, NVDA and JAWS) is listed in
  the README under *Still not verified*.
- [x] **Accessibility** · 🟡
  Screen-reader names and roles for the bar and flyout buttons (UI
  Automation), visible keyboard focus, and a high-contrast theme that
  follows Windows' contrast setting.
