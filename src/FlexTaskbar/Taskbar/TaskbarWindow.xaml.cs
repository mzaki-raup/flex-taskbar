using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Automation;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;
using System.Threading;
using System.Threading.Tasks;
using FlexTaskbar.Applications;
using FlexTaskbar.Menus;
using FlexTaskbar.Native;
using FlexTaskbar.Services;
using FlexTaskbar.Settings;
using FlexTaskbar.Tray;
using FlexTaskbar.Utilities;
using FlexTaskbar.Views;

namespace FlexTaskbar.Taskbar;

/// <summary>
/// The main FlexTaskbar surface. Docks to the bottom of the primary monitor's work
/// area. By default it's a plain topmost window that sits above the real Windows
/// taskbar without reserving any screen space (spec Section 2's safety default);
/// Phase 6 adds the *capability* to register as a real Windows AppBar
/// (<see cref="TaskbarLayoutManager"/>) and to auto-hide, but both are off by
/// default and reachable only via the right-click menu on the taskbar background —
/// there is no Settings UI yet (Phase 7), and this is deliberately not wired into
/// default startup given it changes shared desktop state, not just FlexTaskbar's own
/// window (see TaskbarLayoutManager's doc comment).
///
/// Phase 3 replaces the flat "Apps" popup with a real category system: one button per
/// root category opening a native nested flyout menu, plus an "All" button listing
/// only uncategorized apps — which doubles as the drag source for sorting apps into
/// categories (drop targets are the category buttons themselves).
/// </summary>
public partial class TaskbarWindow : Window
{
    private const double CollapsedHeight = 4.0; // auto-hide "hot edge" thickness

    // Drives periodic battery-status polling only now — the clock/date display and
    // the mirrored system tray icons were removed (round feedback: "remove system
    // tray icon apps, date and time"), but battery still needs a periodic refresh.
    private readonly DispatcherTimer _statusPollTimer;
    private readonly DispatcherTimer _autoHideTimer;
    private readonly DispatcherTimer _appBarHealthTimer;
    private readonly DispatcherTimer _topmostTimer;
    private readonly DispatcherTimer _hoverCloseTimer;

    // Set while dragging an app tile within an open category flyout (round
    // feedback: "when i try to drag and drop the apps inside category, it just
    // close the popup") — DragDrop.DoDragDrop's modal loop still pumps the
    // Dispatcher, so _hoverCloseTimer keeps ticking during the drag, and starting
    // the drag itself triggers MouseLeave on the anchor button/popup border (mouse
    // capture moves to the drag-drop operation) which would otherwise re-arm the
    // timer and close the very popup the drop targets live in. Guards both
    // MouseLeave handlers below so they're inert for the duration of the drag.
    private bool _suppressHoverClose;

    private readonly ApplicationManager _applicationManager = new();
    private readonly CategoryManager _categoryManager = new();
    private readonly SettingsManager _settingsManager = new();
    private readonly ObservableCollection<ApplicationEntry> _uncategorizedApps = new();

    private CategoryMenuBuilder? _categoryMenuBuilder;
    private GlobalHotkeyService? _hotkeyService;
    private GlobalHotkeyService? _recoveryHotkeyService;
    private LauncherWindow? _launcherWindow;
    private SettingsWindow? _settingsWindow;
    private RecoveryWindow? _recoveryWindow;
    private TaskbarLayoutManager? _layoutManager;
    private TrayIconService? _trayIconService;
    private Popup? _hoverPopup;
    private Border? _reorderIndicator;
    private Border? _categoryReorderIndicator;
    private HwndSource? _shellMessageSource;
    private EventWaitHandle? _restartSignalHandle;
    private RegisteredWaitHandle? _restartSignalRegistration;
    private uint _taskbarCreatedMessageId;
    private Point _dragStartPoint;
    private User32.WinEventDelegate? _shellPopupEventCallback;
    private IntPtr _shellForegroundHook;
    private IntPtr _shellObjectHook;
    private readonly HashSet<IntPtr> _visibleShellPopups = new();
    private bool _appsListBoxDragging;
    private bool _autoHideEnabled;
    private bool _isExpanded = true;
    private double _configuredHeight = 48.0; // matches the default Windows 11 taskbar height at 100% scaling
    private TaskbarPosition _position = TaskbarPosition.Bottom;

    internal ApplicationManager ApplicationManagerInstance => _applicationManager;
    internal CategoryManager CategoryManagerInstance => _categoryManager;
    internal SettingsManager SettingsManagerInstance => _settingsManager;
    internal bool AutoHideEnabled => _autoHideEnabled;
    internal bool IsReservingScreenSpace => _layoutManager?.IsRegistered ?? false;
    internal TaskbarPosition CurrentPosition => _position;
    internal double CurrentHeight => _configuredHeight;

    public TaskbarWindow()
    {
        InitializeComponent();

        _settingsManager.Load();
        _configuredHeight = _settingsManager.Current.TaskbarHeight;
        _position = Enum.TryParse<TaskbarPosition>(_settingsManager.Current.Position, out var savedPosition)
            ? savedPosition
            : TaskbarPosition.Bottom;

        ApplyDockedPositionNonAppBar(_configuredHeight);

        _layoutManager = new TaskbarLayoutManager(this) { BarThickness = _configuredHeight, Position = _position };

        _autoHideTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(500) };
        _autoHideTimer.Tick += (_, _) =>
        {
            _autoHideTimer.Stop();
            Collapse();
        };

        // Defense-in-depth for Explorer-restart tolerance (Section 46), alongside
        // the WM_TASKBARCREATED hook below. Live testing showed the broadcast
        // message alone isn't reliably delivered/handled in this environment — after
        // restarting Explorer with AppBar reservation active, the reservation was
        // silently dropped and never re-claimed. A plain ApplyPosition() (QUERYPOS/
        // SETPOS) isn't enough to recover from that: Explorer forgot our ABM_NEW
        // registration entirely, so QUERYPOS/SETPOS on an hwnd it no longer
        // recognizes as a registered AppBar has nothing to act on. HandleShellRestarted()
        // does the full re-registration (ABM_NEW again), which is what's actually
        // needed. Calling it periodically even when nothing is wrong is intentional —
        // there's no official "is my hwnd still a registered AppBar?" query, so
        // rather than depend on detecting the loss, this just re-asserts on a timer.
        _appBarHealthTimer = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromSeconds(20),
        };
        _appBarHealthTimer.Tick += (_, _) => _layoutManager?.HandleShellRestarted();
        _appBarHealthTimer.Start();

        // WPF's Topmost="True" (set in XAML) only asserts the topmost z-order band
        // once; other apps setting themselves topmost afterward can still end up
        // drawn above the taskbar. Re-issuing HWND_TOPMOST periodically is the
        // standard fix — cheap (one SetWindowPos call) and keeps the taskbar
        // reliably on top without stealing focus (SWP_NOACTIVATE).
        //
        // Interval was 3 seconds; round feedback ("windows still show below flex
        // taskbar", "sometime the windows from the hover [go] under below other
        // opened apps" — screenshotted with a visibly corrupted/garbled live
        // thumbnail) pointed at the reassertion itself as the disruptive cause,
        // not just a benign z-order race that better popup-detection could dodge:
        // SetWindowPos(HWND_TOPMOST) forces Explorer/DWM to recompute z-order,
        // and firing that every 3 seconds gave it ample opportunity to land
        // mid-composition of a live taskbar thumbnail (which DWM renders as an
        // actual real-time surface, not a static bitmap) and corrupt/misplace it
        // — explaining both symptoms, including one that plain z-order timing
        // couldn't: the preview ending up behind an ordinary, non-topmost window
        // too. Several rounds of trying to more precisely detect and dodge every
        // kind of shell popup (still left in place below as extra insurance)
        // couldn't fix this, because the popup-detection angle was solving the
        // wrong problem — the fix is calling this far less often, not smarter.
        // 45s still recovers from another app grabbing topmost within a minute,
        // which is what this was ever actually defending against.
        _topmostTimer = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromSeconds(45),
        };
        _topmostTimer.Tick += (_, _) => ReassertTopmost();
        _topmostTimer.Start();

        _hoverCloseTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(250) };
        _hoverCloseTimer.Tick += (_, _) =>
        {
            _hoverCloseTimer.Stop();
            if (!_suppressHoverClose)
                _hoverPopup?.SetCurrentValue(Popup.IsOpenProperty, false);
        };

        AppsListBox.ItemsSource = _uncategorizedApps;

        _statusPollTimer = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromSeconds(1),
        };
        Loaded += TaskbarWindow_Loaded;
        Closed += TaskbarWindow_Closed;
    }

    private async void TaskbarWindow_Loaded(object sender, RoutedEventArgs e)
    {
        _statusPollTimer.Start();
        ReassertTopmost();
        ExcludeFromAltTab();
        InitializeStatusIndicators();

        _categoryMenuBuilder = new CategoryMenuBuilder(_applicationManager, _categoryManager, this);
        _categoryManager.Initialize();
        _categoryManager.Changed += RefreshCenterPanel; // center panel shows category icons — round-3/4 feedback
        _applicationManager.Changed += RefreshUncategorizedApps;
        _applicationManager.Changed += RefreshCenterPanel; // pin/unpin must repaint the center panel immediately

        RefreshCenterPanel();
        InitializeHotkey();
        InitializeRecoveryHotkey();
        InitializeTrayIcon();
        InitializeShellRestartHandling();
        InitializeRestartSignalListener();
        InitializeShellPopupWatcher();

        // Apply persisted preferences that need real side effects, not just a stored
        // value (Section 29's General settings). Both are still off unless the user
        // previously turned them on via Settings or the right-click menu — the
        // *default* AppSettings values are false, so a first-ever run is unaffected.
        if (_settingsManager.Current.AutoHide)
            SetAutoHide(true);
        if (_settingsManager.Current.ReserveScreenSpace)
            ToggleReserveScreenSpace();

        try
        {
            await _applicationManager.InitializeAsync();
        }
        catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException)
        {
            // A failed scan must not take the taskbar down with it (Section 34) —
            // the "All" popup and category menus will just show whatever was cached.
        }
    }

    private void TaskbarWindow_Closed(object? sender, EventArgs e)
    {
        _statusPollTimer.Stop();
        _autoHideTimer.Stop();
        _appBarHealthTimer.Stop();
        _topmostTimer.Stop();
        _hoverCloseTimer.Stop();
        _hotkeyService?.Dispose();
        _recoveryHotkeyService?.Dispose();
        _layoutManager?.Dispose(); // must run so a reserved AppBar strip doesn't outlive the process
        _trayIconService?.Dispose();

        if (_shellMessageSource is not null)
            _shellMessageSource.RemoveHook(ShellMessageWndProc);

        if (_shellForegroundHook != IntPtr.Zero)
            User32.UnhookWinEvent(_shellForegroundHook);
        if (_shellObjectHook != IntPtr.Zero)
            User32.UnhookWinEvent(_shellObjectHook);

        _restartSignalRegistration?.Unregister(null);
        _restartSignalHandle?.Dispose();
    }

    private void InitializeRecoveryHotkey()
    {
        // Distinct id from the launcher hotkey's GlobalHotkeyService instance —
        // RegisterHotKey requires a unique id per HWND (see GlobalHotkeyService's
        // hotkeyId parameter doc).
        _recoveryHotkeyService = new GlobalHotkeyService(this, hotkeyId: 0x4A47);
        _recoveryHotkeyService.HotkeyPressed += OpenRecovery;

        var registered = _recoveryHotkeyService.Register(
            User32.MOD_CONTROL | User32.MOD_ALT | User32.MOD_SHIFT | User32.MOD_NOREPEAT,
            User32.VK_F12);

        if (!registered)
        {
            Console.Error.WriteLine("[FlexTaskbar] Recovery hotkey (Ctrl+Alt+Shift+F12) registration failed — already claimed by another app. Recovery is still reachable via the right-click menu's Settings item, or FlexTaskbar.exe --safe-mode.");
        }
    }

    internal void OpenRecovery()
    {
        if (_recoveryWindow is { IsVisible: true })
        {
            _recoveryWindow.Activate();
            return;
        }

        _recoveryWindow = new RecoveryWindow(this, isSafeMode: false);
        _recoveryWindow.Closed += (_, _) => _recoveryWindow = null;
        _recoveryWindow.Show();
        _recoveryWindow.Activate();
    }

    /// <summary>
    /// Explorer-restart tolerance (spec Section 46): listens for the well-known
    /// "TaskbarCreated" broadcast Explorer sends every top-level window when it
    /// (re)starts, and re-claims the AppBar reservation if one was active — Explorer
    /// owns the registered-AppBar list, so a restart silently drops it otherwise.
    /// </summary>
    private void InitializeShellRestartHandling()
    {
        _taskbarCreatedMessageId = User32.RegisterWindowMessage("TaskbarCreated");
        var hwnd = new WindowInteropHelper(this).Handle;
        _shellMessageSource = HwndSource.FromHwnd(hwnd);
        _shellMessageSource?.AddHook(ShellMessageWndProc);
    }

    private IntPtr ShellMessageWndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (_taskbarCreatedMessageId != 0 && msg == (int)_taskbarCreatedMessageId)
        {
            _layoutManager?.HandleShellRestarted();
        }

        return IntPtr.Zero;
    }

    /// <summary>
    /// Listens for another process invoking <c>--restart</c>/<c>--reset</c> and
    /// closes this instance gracefully in response, so the new process can then
    /// proceed past the single-instance mutex check (spec Section 54/55).
    /// </summary>
    private void InitializeRestartSignalListener()
    {
        _restartSignalHandle = InstanceSignalingService.CreateListener();
        _restartSignalRegistration = ThreadPool.RegisterWaitForSingleObject(
            _restartSignalHandle,
            (_, _) => Dispatcher.BeginInvoke(Close),
            null,
            Timeout.Infinite,
            executeOnlyOnce: false);
    }

    private void InitializeHotkey()
    {
        _hotkeyService = new GlobalHotkeyService(this);
        _hotkeyService.HotkeyPressed += OpenLauncher;
        ApplyHotkeyPreset(_settingsManager.Current.HotkeyPreset, logFailure: true);
    }

    /// <summary>Registers the given preset, logging (not throwing) on failure — see
    /// GlobalHotkeyService's own doc comment for why a lost OS-level claim is expected,
    /// not exceptional. Used both at startup and when Settings changes the preset.</summary>
    internal bool ApplyHotkeyPreset(string presetName, bool logFailure = false)
    {
        if (_hotkeyService is null)
            return false;

        var (modifiers, virtualKey) = HotkeyPresets.Resolve(presetName);
        var success = _hotkeyService.Register(modifiers, virtualKey);

        if (!success && logFailure)
        {
            Console.Error.WriteLine($"[FlexTaskbar] Hotkey registration failed for preset '{presetName}' (likely already claimed by Windows or another app). Use the \"All\" button or a category shortcut on the taskbar instead.");
        }

        return success;
    }

    private void InitializeTrayIcon()
    {
        _trayIconService = new TrayIconService();
        _trayIconService.OpenSettingsRequested += OpenSettings;
        _trayIconService.RescanRequested += async () =>
        {
            try
            {
                await _applicationManager.RescanAsync();
            }
            catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException)
            {
                // Same "never crash over a scan failure" contract as the initial scan.
            }
        };
        _trayIconService.ExitRequested += Close;

        // See TrayIconService.ContextMenuOpened's doc comment — without this, the
        // periodic re-assert-topmost timer could fire while the tray menu is open
        // and cover it back up (round feedback: "make the windows 11 system tray
        // right click menu on top of the flextaskbar").
        _trayIconService.ContextMenuOpened += _topmostTimer.Stop;
        _trayIconService.ContextMenuClosed += _topmostTimer.Start;
    }

    internal void OpenSettings()
    {
        if (_settingsWindow is { IsVisible: true })
        {
            _settingsWindow.Activate();
            return;
        }

        _settingsWindow = new SettingsWindow(this) { Owner = this };
        _settingsWindow.Closed += (_, _) => _settingsWindow = null;
        _settingsWindow.Show();
        _settingsWindow.Activate();
    }

    /// <summary>Applies a new position/height from Settings and persists it.</summary>
    internal void ApplyPositionAndHeight(TaskbarPosition position, double height)
    {
        _position = position;
        _configuredHeight = height;

        if (_layoutManager is not null)
            _layoutManager.Position = position;

        if (_isExpanded)
            ApplyHeight(_configuredHeight);

        _settingsManager.Current.Position = position.ToString();
        _settingsManager.Current.TaskbarHeight = height;
        _settingsManager.Save();
    }

    internal void ApplyAutoHideSetting(bool enabled)
    {
        SetAutoHide(enabled);
        _settingsManager.Current.AutoHide = enabled;
        _settingsManager.Save();
    }

    internal void ApplyReserveScreenSpaceSetting(bool enabled)
    {
        if (enabled != IsReservingScreenSpace)
            ToggleReserveScreenSpace();

        _settingsManager.Current.ReserveScreenSpace = enabled;
        _settingsManager.Save();
    }

    private void OpenLauncher()
    {
        if (_launcherWindow is { IsVisible: true })
        {
            _launcherWindow.Activate();
            return;
        }

        if (_categoryMenuBuilder is null)
            return;

        _launcherWindow = new LauncherWindow(_applicationManager, _categoryManager, _categoryMenuBuilder, OpenSettings);
        _launcherWindow.PositionAboveTaskbar(SystemParameters.WorkArea.Left + 8, Top);
        _launcherWindow.Closed += (_, _) => _launcherWindow = null;
        _launcherWindow.Show();
        _launcherWindow.Activate();
    }

    // --- Status indicators (battery — spec Section 14; network/volume removed
    // per round feedback: "remove also network and volume mixer icon on the far
    // right") ---

    private void InitializeStatusIndicators()
    {
        RefreshBatteryStatus();
        _statusPollTimer.Tick += (_, _) => RefreshBatteryStatus();
    }

    private void RefreshBatteryStatus()
    {
        var battery = SystemStatusService.GetBatteryStatus();
        if (battery is null)
        {
            BatteryStatusButton.Visibility = Visibility.Collapsed; // no battery (desktop) — don't show a fake indicator
            return;
        }

        BatteryStatusButton.Visibility = Visibility.Visible;
        var (percent, charging) = battery.Value;
        BatteryStatusButton.Content = (charging ? "🔌 " : "🔋 ") + percent + "%";
        BatteryStatusButton.ToolTip = (charging ? "Charging, " : "") + percent + "% — click for power settings";
    }

    private void BatteryStatusButton_Click(object sender, RoutedEventArgs e) => TryOpenSettingsUri("ms-settings:batterysaver");

    private void SettingsButton_Click(object sender, RoutedEventArgs e) => OpenSettings();

    private static void TryOpenSettingsUri(string uri)
    {
        try
        {
            Process.Start(new ProcessStartInfo { FileName = uri, UseShellExecute = true });
        }
        catch (System.ComponentModel.Win32Exception)
        {
            // Non-fatal — worst case the shortcut just doesn't do anything this time.
        }
    }

    /// <summary>Deferred for the same reason <see cref="RefreshCenterPanel"/> is —
    /// this is subscribed to ApplicationManager's Changed event, which "assign
    /// this app to a category" fires from inside the still-active
    /// DragDrop.DoDragDrop call for the AppsListBox item being dragged (see
    /// AppsListBox_PreviewMouseMove). Mutating the bound _uncategorizedApps
    /// collection synchronously at that point would regenerate the ListBoxItem
    /// that's still WPF's active drag source mid-operation.</summary>
    private void RefreshUncategorizedApps() => Dispatcher.BeginInvoke(RefreshUncategorizedAppsCore, DispatcherPriority.Background);

    private void RefreshUncategorizedAppsCore()
    {
        _uncategorizedApps.Clear();
        foreach (var app in _applicationManager.Applications
                     .Where(a => a.CategoryId is null)
                     .OrderBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase))
        {
            _uncategorizedApps.Add(app);
        }
    }

    /// <summary>Builds a category's icon button for the center panel (round-4
    /// feedback removed the redundant left-side copy that used to also exist in
    /// its own CategoriesPanel — this is the only place category icons render
    /// now, blended with running/pinned app icons).</summary>
    private Button BuildCategoryButton(ApplicationCategory category)
    {
        var button = new Button
        {
            Content = BuildCategoryIconContent(category),
            Style = (Style)FindResource("TaskbarButtonStyle"),
            Tag = category,
            AllowDrop = true,
            ToolTip = category.Name,
        };
        AutomationProperties.SetName(button, category.Name);
        button.Click += (_, _) => ShowCategoryFlyout(button, category);
        button.DragEnter += CategoryButton_DragOver;
        button.DragOver += CategoryButton_DragOver;
        button.Drop += CategoryButton_Drop;
        AttachHoverCategoryFlyout(button, category);

        // Drag source for reordering root categories among themselves (round
        // feedback: "apps shortcut and category can be rearranged in order by drag
        // and drop") — closes any open hover flyout first so it doesn't linger
        // mid-drag. Carries the category itself (not an ApplicationEntry), so
        // CategoryButton_Drop can tell a reorder apart from "assign this app to
        // this category".
        AttachDragSource(button, category, () => _hoverPopup?.SetCurrentValue(Popup.IsOpenProperty, false));

        return button;
    }

    /// <summary>Opens the category flyout on hover, not just click (round feedback:
    /// "follow when hover on the category only, it will show apps inside it") — a
    /// short close delay (shared <see cref="_hoverCloseTimer"/>) so moving the
    /// mouse from the button up into the flyout doesn't close it prematurely. Click
    /// still opens/refreshes it too, for keyboard/touch access.</summary>
    private void AttachHoverCategoryFlyout(Button button, ApplicationCategory category)
    {
        button.MouseEnter += (_, _) =>
        {
            _hoverCloseTimer.Stop();
            ShowCategoryFlyout(button, category);
        };
        button.MouseLeave += (_, _) =>
        {
            _hoverCloseTimer.Stop();
            if (!_suppressHoverClose)
                _hoverCloseTimer.Start();
        };
    }

    /// <summary>
    /// Category flyout, rebuilt as a plain Popup (like <see cref="ShowHoverWindowList"/>)
    /// instead of a native ContextMenu/MenuItem tree — a MenuItem's own selection
    /// chrome was rendering as a translucent bar behind/around the app tiles
    /// (round feedback: "it seem like have transparency bar"), since a MenuItem
    /// always highlights across its own full row width regardless of how narrow its
    /// Header content actually is. Apps render as icon+title tiles, sized and
    /// styled identically to the multi-window hover picker
    /// (<see cref="ShowHoverWindowList"/> — round feedback: "same like horizontal
    /// tile e.g. multiple chrome windows horizontal tile") so both tile grids look
    /// and behave the same way. Subcategories are small pill buttons that drill in
    /// by re-rendering this same popup for the subcategory.
    /// </summary>
    private void ShowCategoryFlyout(Button anchor, ApplicationCategory category)
    {
        _hoverPopup?.SetCurrentValue(Popup.IsOpenProperty, false);

        var content = new StackPanel { Margin = new Thickness(4) };

        var subcategories = _categoryManager.GetChildren(category.Id).ToList();
        if (subcategories.Count > 0)
        {
            var subRow = new WrapPanel { MaxWidth = 360, Margin = new Thickness(0, 0, 0, 4) };
            foreach (var sub in subcategories)
            {
                var subButton = new Button
                {
                    Content = $"{(string.IsNullOrEmpty(sub.IconGlyph) ? "📁" : sub.IconGlyph)} {sub.Name}",
                    Style = (Style)FindResource("TaskbarButtonStyle"),
                    Margin = new Thickness(0, 0, 4, 4),
                };
                subButton.Click += (_, _) => ShowCategoryFlyout(anchor, sub);
                subRow.Children.Add(subButton);
            }
            content.Children.Add(subRow);
        }

        var apps = _applicationManager.GetApplicationsInCategory(category.Id).ToList();

        if (apps.Count > 0)
        {
            // The WrapPanel itself (not each tile individually) is the drop
            // target/authority for reordering, same reasoning as the taskbar's
            // center panel: a tile that only knows how to react to drops on its
            // own exact bounds makes reordering feel broken the moment the drop
            // lands one pixel off.
            var wrapPanel = new WrapPanel { MaxWidth = 360, AllowDrop = true };
            wrapPanel.DragEnter += (_, e) => CategoryAppWrapPanel_DragOver(wrapPanel, e);
            wrapPanel.DragOver += (_, e) => CategoryAppWrapPanel_DragOver(wrapPanel, e);
            wrapPanel.DragLeave += (_, _) => RemoveCategoryReorderIndicator(wrapPanel);
            wrapPanel.Drop += (_, e) => CategoryAppWrapPanel_Drop(wrapPanel, e, anchor, category);
            foreach (var app in apps)
                wrapPanel.Children.Add(BuildCategoryAppTile(app, anchor, category));
            content.Children.Add(wrapPanel);
        }
        else if (subcategories.Count == 0)
        {
            content.Children.Add(new TextBlock
            {
                Text = "No apps in this category",
                FontSize = 11,
                Margin = new Thickness(4),
                Foreground = (Brush)FindResource("TaskbarSubtleForegroundBrush"),
            });
        }

        var manageButton = new Button
        {
            Content = "⚙ Manage Category",
            Style = (Style)FindResource("TaskbarButtonStyle"),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Left,
            Margin = new Thickness(0, 4, 0, 0),
        };
        manageButton.Click += (_, _) =>
        {
            _hoverPopup?.SetCurrentValue(Popup.IsOpenProperty, false);
            var window = new CategoryManagementWindow(category, _categoryManager, _applicationManager) { Owner = this };
            window.ShowDialog();
        };
        content.Children.Add(manageButton);

        var border = new Border
        {
            Background = (Brush)FindResource("TaskbarBackgroundBrush"),
            BorderBrush = (Brush)FindResource("TaskbarBorderBrush"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(6),
            Child = content,
            // Popup content is its own separate visual-tree root — it doesn't
            // inherit UseLayoutRounding/SnapsToDevicePixels from TaskbarWindow's
            // own root, even though those are set there. Without them here, icons
            // in this flyout can land on sub-pixel boundaries and blur regardless
            // of source resolution or BitmapScalingMode (round feedback, still
            // persisting after those two fixes: "still not sharp").
            UseLayoutRounding = true,
            SnapsToDevicePixels = true,
        };
        border.MouseEnter += (_, _) => _hoverCloseTimer.Stop();
        border.MouseLeave += (_, _) =>
        {
            _hoverCloseTimer.Stop();
            if (!_suppressHoverClose)
                _hoverCloseTimer.Start();
        };

        _hoverPopup = new Popup
        {
            PlacementTarget = anchor,
            Placement = PlacementMode.Top,
            AllowsTransparency = true,
            Child = border,
            IsOpen = true,
        };
    }

    /// <summary>Same tile shape as the multi-window hover picker's tiles — icon
    /// above, name below, 84x76 — so the two tile grids read as one visual
    /// language. Also a drag source for reordering apps within the category (round
    /// feedback: "apps inside category can drag and drop") — the enclosing
    /// WrapPanel, not the tile itself, is the drop target; see
    /// <see cref="CategoryAppWrapPanel_Drop"/>.</summary>
    private Button BuildCategoryAppTile(ApplicationEntry app, Button anchor, ApplicationCategory category)
    {
        var tileContent = new StackPanel { Orientation = Orientation.Vertical, HorizontalAlignment = HorizontalAlignment.Center };
        if (ApplicationIconConverter.IconService.GetIcon(app) is { } icon)
        {
            // Was 28x28 — bumped to match the center panel's own icon-size fix
            // (round feedback: "apply this also inside categories list of apps")
            // so app tiles inside a category flyout read at the same visual
            // weight as pinned shortcuts sitting on the taskbar itself.
            tileContent.Children.Add(new Image { Width = 32, Height = 32, Margin = new Thickness(0, 4, 0, 4), Source = icon });
        }
        tileContent.Children.Add(new TextBlock
        {
            Text = Truncate(app.Name, 20),
            FontSize = 11,
            TextAlignment = TextAlignment.Center,
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 76,
            // Always reserves two lines of height, even for names that only need
            // one — otherwise a short name sits higher than a wrapped one and tiles
            // in the same row look misaligned (round feedback: "make the app name
            // text 2 row of line even the apps name only use 1 row of line").
            MinHeight = 28,
        });

        var tileButton = new Button
        {
            Content = tileContent,
            Style = (Style)FindResource("TaskbarButtonStyle"),
            Width = 84,
            Height = 76,
            Margin = new Thickness(2),
            Tag = app,
            ToolTip = app.Name,
        };
        AutomationProperties.SetName(tileButton, app.Name);
        tileButton.Click += (_, _) =>
        {
            _applicationManager.Launch(app);
            _hoverPopup?.SetCurrentValue(Popup.IsOpenProperty, false);
        };

        // Doesn't close the popup on drag-start (unlike AttachDragSource's use for
        // category buttons) — the tile lives inside _hoverPopup itself, and closing
        // it would remove the very drop targets being dragged toward. Instead
        // suppresses the hover-close timer for the duration of the drag — see
        // _suppressHoverClose's doc comment for why that's needed at all.
        AttachDragSource(
            tileButton,
            app,
            onDragStart: () =>
            {
                _suppressHoverClose = true;
                _hoverCloseTimer.Stop();
            },
            onDragEnd: () => _suppressHoverClose = false);

        return tileButton;
    }

    /// <summary>
    /// Authoritative drop zone for reordering apps within a category (round
    /// feedback: "apps inside category can drag and drop", refined by "i still
    /// cannot drag and drop category to the right side" — the same "a tile only
    /// reacting to drops on its own exact bounds" problem the taskbar's center
    /// panel had — and "put also indicator inside category app to show where the
    /// apps position will be put"). Shows a drop-position indicator, same idea as
    /// <see cref="ShowReorderIndicator"/> for the center panel, but a real sibling
    /// inserted into the WrapPanel's own Children rather than a floating overlay —
    /// this popup isn't centered inside a wider container the way the taskbar row
    /// is, so the reflow-on-insert that caused jank there doesn't apply here.
    /// </summary>
    private void CategoryAppWrapPanel_DragOver(WrapPanel wrapPanel, DragEventArgs e)
    {
        if (!e.Data.GetDataPresent(typeof(ApplicationEntry)))
        {
            RemoveCategoryReorderIndicator(wrapPanel);
            return;
        }

        e.Effects = DragDropEffects.Move;
        ShowCategoryReorderIndicator(wrapPanel, ComputeCategoryInsertionIndex(wrapPanel, e.GetPosition(wrapPanel)));
        e.Handled = true;
    }

    private void CategoryAppWrapPanel_Drop(WrapPanel wrapPanel, DragEventArgs e, Button anchor, ApplicationCategory category)
    {
        if (e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry dragged)
        {
            var insertionIndex = ComputeCategoryInsertionIndex(wrapPanel, e.GetPosition(wrapPanel));
            RemoveCategoryReorderIndicator(wrapPanel);
            _applicationManager.SetCategoryPosition(dragged, category.Id, insertionIndex);

            // The popup's tiles were built once at open time, not data-bound —
            // rebuild in place so the new order/membership is visible immediately
            // instead of only after the flyout is closed and reopened. Deferred
            // (not called synchronously) for the same reason RefreshCenterPanel
            // is now deferred — this rebuild can destroy the exact tile that's
            // still the active DragDrop.DoDragDrop source when reordering within
            // the same category, which corrupted state on real drag tests.
            Dispatcher.BeginInvoke(
                () =>
                {
                    if (_hoverPopup?.IsOpen == true)
                        ShowCategoryFlyout(anchor, category);
                },
                DispatcherPriority.Background);
        }

        e.Handled = true;
    }

    /// <summary>Which tile index (matching <see cref="ApplicationManager.GetApplicationsInCategory"/>'s
    /// order) a drop at <paramref name="mouse"/> would land at — finds the row
    /// closest to the cursor's Y, then the insertion point within that row by X,
    /// same left-to-right "before the first tile whose center is past the cursor"
    /// rule the center panel uses, generalized to a wrapping grid instead of one
    /// row.</summary>
    private static int ComputeCategoryInsertionIndex(WrapPanel wrapPanel, Point mouse)
    {
        var tiles = wrapPanel.Children.OfType<Button>()
            .Select((tile, index) => (tile, index, topLeft: tile.TranslatePoint(new Point(0, 0), wrapPanel)))
            .ToList();

        if (tiles.Count == 0)
            return 0;

        var rowTop = tiles
            .Select(t => t.topLeft.Y)
            .Distinct()
            .OrderBy(y => y)
            .FirstOrDefault(y => mouse.Y < y + tiles[0].tile.ActualHeight, tiles.Max(t => t.topLeft.Y));

        var row = tiles.Where(t => t.topLeft.Y == rowTop).OrderBy(t => t.topLeft.X).ToList();

        foreach (var item in row)
        {
            if (mouse.X < item.topLeft.X + item.tile.ActualWidth / 2)
                return item.index;
        }

        return row[^1].index + 1;
    }

    private void ShowCategoryReorderIndicator(WrapPanel wrapPanel, int index)
    {
        _categoryReorderIndicator ??= new Border
        {
            Width = 3,
            Height = 76,
            Margin = new Thickness(2),
            Background = (Brush)FindResource("AccentBrush"),
        };

        wrapPanel.Children.Remove(_categoryReorderIndicator);
        wrapPanel.Children.Insert(Math.Min(index, wrapPanel.Children.Count), _categoryReorderIndicator);
    }

    private void RemoveCategoryReorderIndicator(WrapPanel wrapPanel)
    {
        if (_categoryReorderIndicator is not null)
            wrapPanel.Children.Remove(_categoryReorderIndicator);
    }

    /// <summary>
    /// Only claims "categorize an app that isn't pinned yet into this category" —
    /// a specific-button target. A category being dragged, or an already-pinned
    /// app being dragged (both mean "reorder", not "categorize"), is deliberately
    /// left unhandled here so the event bubbles up to
    /// <see cref="CenterPanel_DragOver"/> — round feedback ("i can't put the apps
    /// shortcut on the left of category icon") traced back to this handler
    /// unconditionally claiming *every* ApplicationEntry drag regardless of pin
    /// state, so a pinned shortcut dropped anywhere near a category button always
    /// got re-categorized instead of ever reaching the center panel's reorder
    /// logic — the button was, in effect, unreachable as a reorder target no
    /// matter how precisely you aimed. Categorizing an already-pinned app is still
    /// possible — just via its context menu's "Move to Category" now (see
    /// <see cref="BuildPinnedAppContextMenu"/>) instead of drag-drop, since that
    /// gesture is needed for reordering instead.
    /// </summary>
    private void CategoryButton_DragOver(object sender, DragEventArgs e)
    {
        if (e.Data.GetDataPresent(typeof(ApplicationEntry)) &&
            e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry { IsPinned: false })
        {
            e.Effects = DragDropEffects.Move;
            e.Handled = true;
        }
    }

    private void CategoryButton_Drop(object sender, DragEventArgs e)
    {
        if (sender is Button { Tag: ApplicationCategory category } &&
            e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry { IsPinned: false } entry)
        {
            _applicationManager.SetCategory(entry, category.Id);
            e.Handled = true; // don't let this bubble into Taskbar_Drop and also pin the app
        }
        // Pinned entries and ApplicationCategory payloads: deliberately not handled — bubble to CenterPanel_Drop.
    }

    /// <summary>
    /// Pinning a shortcut to the taskbar (round-3 feedback: "user can add apps
    /// shortcut on the taskbar... drag and drop shortcut to the taskbar"). Handles
    /// two distinct drag sources: an internal drag of an <see cref="ApplicationEntry"/>
    /// from the "All Applications" popup (same DragDrop mechanism category buttons
    /// already use), and a real external .lnk/.exe file dragged in from File
    /// Explorer or the desktop (<see cref="DataFormats.FileDrop"/>).
    /// </summary>
    private void Taskbar_DragEnter(object sender, DragEventArgs e)
    {
        e.Effects = e.Data.GetDataPresent(typeof(ApplicationEntry)) || e.Data.GetDataPresent(DataFormats.FileDrop)
            ? DragDropEffects.Move
            : DragDropEffects.None;
        e.Handled = true;
    }

    private void Taskbar_Drop(object sender, DragEventArgs e)
    {
        if (e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry entry)
        {
            _applicationManager.SetPinned(entry, true);
            return;
        }

        if (e.Data.GetData(DataFormats.FileDrop) is string[] files)
        {
            foreach (var file in files)
                _applicationManager.PinShortcutFile(file);
        }
    }

    private void NewCategoryButton_Click(object sender, RoutedEventArgs e)
    {
        var name = TextInputWindow.Prompt(this, "New category name:");
        if (name is not null)
            _categoryManager.AddCategory(name, null);
    }

    private void AllAppsButton_Click(object sender, RoutedEventArgs e)
    {
        AppsPopup.IsOpen = !AppsPopup.IsOpen;
    }

    /// <summary>
    /// Drag-to-unpin — the reverse of dragging an entry from the "All Applications"
    /// popup onto the taskbar to pin it (see <see cref="Taskbar_Drop"/>). Dropping a
    /// pinned/running app's icon (see <see cref="AttachUnpinDragSource"/>) back onto
    /// the "All" button unpins it, putting it back in the All Applications list —
    /// the right-click "Unpin from taskbar" menu item already did this, this just
    /// gives it a drag gesture symmetric with how pinning itself works.
    /// </summary>
    private void AllAppsButton_DragOver(object sender, DragEventArgs e)
    {
        e.Effects = e.Data.GetDataPresent(typeof(ApplicationEntry)) ? DragDropEffects.Move : DragDropEffects.None;
        e.Handled = true;
    }

    private void AllAppsButton_Drop(object sender, DragEventArgs e)
    {
        if (e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry entry)
        {
            _applicationManager.SetPinned(entry, false);
            AppsPopup.IsOpen = true; // show it landing back in the list, same spirit as opening on click
        }

        e.Handled = true; // don't let this bubble into Taskbar_Drop and re-pin what we just unpinned
    }

    /// <summary>
    /// Makes a pinned app's taskbar button a drag source for the gesture above —
    /// only meaningful (and only wired up) for apps that are actually pinned, since
    /// dragging an ordinary running-but-unpinned window's icon onto "All" wouldn't
    /// mean anything. Threshold-gated the same way as the All Applications list's own
    /// drag source (<see cref="AppsListBox_PreviewMouseMove"/>) so an ordinary click
    /// (activate/minimize/launch) isn't misread as a drag.
    /// </summary>
    private static void AttachUnpinDragSource(Button button, ApplicationEntry? entry)
    {
        if (entry is null || !entry.IsPinned)
            return;

        // Same payload type (ApplicationEntry) the "reorder pinned shortcuts" drop
        // target (PinnedAppButton_Drop) and the "unpin" drop target (AllAppsButton_Drop)
        // both already expect — which drop target it lands on decides what happens.
        AttachDragSource(button, entry);
    }

    /// <summary>
    /// Generic press-and-drag gesture source, threshold-gated so an ordinary click
    /// isn't misread as a drag (same reasoning as the All Applications list's own
    /// drag source, <see cref="AppsListBox_PreviewMouseMove"/>). Used for dragging
    /// both app entries (pin/unpin/categorize/reorder — round-3/4 feedback and
    /// round feedback: "apps shortcut and category can be rearranged in order by
    /// drag and drop") and categories (reordering the same round feedback covers).
    /// </summary>
    private static void AttachDragSource(Button button, object payload, Action? onDragStart = null, Action? onDragEnd = null)
    {
        Point? dragStart = null;
        button.PreviewMouseLeftButtonDown += (_, e) => dragStart = e.GetPosition(null);
        button.PreviewMouseMove += (_, e) =>
        {
            if (dragStart is not { } start || e.LeftButton != MouseButtonState.Pressed)
                return;

            var delta = start - e.GetPosition(null);
            if (Math.Abs(delta.X) < SystemParameters.MinimumHorizontalDragDistance &&
                Math.Abs(delta.Y) < SystemParameters.MinimumVerticalDragDistance)
            {
                return;
            }

            dragStart = null;
            onDragStart?.Invoke();
            try
            {
                DragDrop.DoDragDrop(button, payload, DragDropEffects.Move);
            }
            finally
            {
                onDragEnd?.Invoke();
            }
        };
    }

    /// <summary>Manually pins a web app/PWA by URL (spec: "allow apps from web
    /// (Chrome, Edge) to be on the taskbar") — for sites without an existing
    /// Chrome/Edge-installed-PWA shortcut for ApplicationScanner to discover
    /// automatically.</summary>
    /// <summary>
    /// Handles four distinct target forms (round-5 feedback: "for the web apps,
    /// if I want to open the web apps like these example, is it possible?" —
    /// citing a Chrome PWA launched via chrome_proxy.exe --app-id, and a
    /// shell:AppsFolder AUMID launch for an installed PWA/UWP app; later extended
    /// to registered URI protocol handlers — round feedback: "when i want to add
    /// ms-screenclip:, it say not found. This is snipping tool"):
    /// - An http(s):// URL → <see cref="ApplicationManager.AddManualWebApp"/>, unchanged.
    /// - A "shell:..." target (e.g. "shell:AppsFolder\discord.com-...!App"), or
    ///   any other registered URI protocol like "ms-screenclip:" →
    ///   <see cref="ApplicationManager.AddManualShortcut"/>, no arguments prompt
    ///   since neither AppsFolder nor protocol-URI launches take any (the whole
    ///   target string is what gets launched).
    /// - Anything else → treated as an executable path, with a further prompt for
    ///   optional arguments (e.g. chrome_proxy.exe's --profile-directory/--app-id).
    /// </summary>
    private void AddWebAppButton_Click(object sender, RoutedEventArgs e)
    {
        var name = TextInputWindow.Prompt(this, "Name:");
        if (name is null)
            return;

        var target = TextInputWindow.Prompt(this,
            "URL (https://...), executable path, shell:AppsFolder\\..., or a protocol like ms-screenclip: :");
        if (target is null)
            return;

        try
        {
            if (Uri.TryCreate(target, UriKind.Absolute, out var uri) &&
                (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps))
            {
                _applicationManager.AddManualWebApp(name, target);
                return;
            }

            if (target.StartsWith("shell:", StringComparison.OrdinalIgnoreCase) || IsProtocolUri(target))
            {
                _applicationManager.AddManualShortcut(name, target, string.Empty);
                return;
            }

            var arguments = TextInputWindow.Prompt(this, "Arguments (optional):") ?? string.Empty;
            _applicationManager.AddManualShortcut(name, target, arguments);
        }
        catch (ArgumentException ex)
        {
            MessageBox.Show(this, ex.Message, "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    /// <summary>Same check as ApplicationManager's own (private) IsProtocolUri —
    /// kept in sync manually since the two classes don't share a base, same as
    /// BuildIdSeed elsewhere in this codebase. Only used here to skip the pointless
    /// "Arguments" prompt for a protocol URI; ApplicationManager.AddManualShortcut
    /// re-derives the same classification on its own regardless, so this being out
    /// of sync would affect the prompt shown, never correctness.</summary>
    private static bool IsProtocolUri(string target) =>
        Uri.TryCreate(target, UriKind.Absolute, out var uri) &&
        uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps &&
        !string.Equals(uri.Scheme, Uri.UriSchemeFile, StringComparison.OrdinalIgnoreCase);

    private void AppsListBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        // Deliberately does nothing with the selection itself — see
        // AppsListBox_PreviewMouseLeftButtonUp for why launching happens there
        // instead. Just clear it so nothing stays visually highlighted.
        AppsListBox.SelectedItem = null;
    }

    private void AppsListBox_PreviewMouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        _dragStartPoint = e.GetPosition(null);
        _appsListBoxDragging = false;
    }

    private void AppsListBox_PreviewMouseMove(object sender, MouseEventArgs e)
    {
        if (e.LeftButton != MouseButtonState.Pressed || _appsListBoxDragging)
            return;

        var current = e.GetPosition(null);
        var delta = _dragStartPoint - current;
        if (Math.Abs(delta.X) < SystemParameters.MinimumHorizontalDragDistance &&
            Math.Abs(delta.Y) < SystemParameters.MinimumVerticalDragDistance)
        {
            return;
        }

        if (FindAncestorListBoxItem((DependencyObject)e.OriginalSource) is not { } item ||
            item.DataContext is not ApplicationEntry entry)
        {
            return;
        }

        // Mark the gesture as a drag *before* DoDragDrop, which blocks until the
        // drag completes — the subsequent MouseLeftButtonUp (fired as the drop
        // finishes) must not also be treated as a plain click and launch the app.
        _appsListBoxDragging = true;
        DragDrop.DoDragDrop(item, entry, DragDropEffects.Move);
    }

    /// <summary>
    /// Launches the app on mouse-up, not on WPF's own SelectionChanged (which used
    /// to drive launching here). ListBoxItem selects on mouse-*down*, so tying the
    /// launch to SelectionChanged meant the app launched — and the popup closed —
    /// the instant you pressed the mouse button, before PreviewMouseMove ever saw
    /// enough movement to recognize a drag gesture. Reported live: "when i click
    /// apps to drag and drop, it directly open the apps." Waiting for mouse-up,
    /// and skipping the launch entirely when a drag was already recognized (see
    /// _appsListBoxDragging), lets a genuine drag gesture complete instead of
    /// always resolving as an instant click.
    /// </summary>
    private void AppsListBox_PreviewMouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (_appsListBoxDragging)
        {
            _appsListBoxDragging = false;
            return;
        }

        if (FindAncestorListBoxItem((DependencyObject)e.OriginalSource) is not { } item ||
            item.DataContext is not ApplicationEntry entry)
        {
            return;
        }

        _applicationManager.Launch(entry);
        AppsPopup.IsOpen = false;
    }

    /// <summary>Right-click "Pin to taskbar"/"Unpin from taskbar" on an entry in the
    /// "All Applications" popup — a discoverable alternative to the drag-and-drop
    /// pinning gesture (round-3 feedback covers both).</summary>
    private void AppsListBox_MouseRightButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (FindAncestorListBoxItem(e.OriginalSource as DependencyObject) is not { DataContext: ApplicationEntry entry } item)
            return;

        var menu = new ContextMenu { PlacementTarget = item };
        AddMenuAction(menu, entry.IsPinned ? "Unpin from taskbar" : "Pin to taskbar",
            () => _applicationManager.SetPinned(entry, !entry.IsPinned));
        menu.Items.Add(new Separator());
        menu.Items.Add(BuildMoveToCategoryMenuItem(entry));
        menu.Items.Add(new Separator());
        AddMenuAction(menu, "Change Icon...", () => ChangeAppIcon(entry));
        if (entry.CustomIconPath is not null)
            AddMenuAction(menu, "Reset Icon", () => _applicationManager.ResetIcon(entry));
        menu.Items.Add(new Separator());
        // Data field (RunAsAdministrator) already existed for every entry, carried
        // across rescans, and Launch() already honored it via the "runas" verb —
        // but nothing in the UI ever set it. Added after a live request for a
        // manual shortcut (mmc.exe services.msc) that needs to launch elevated.
        var runAsAdminItem = new MenuItem { Header = "Run as administrator", IsCheckable = true, IsChecked = entry.RunAsAdministrator };
        runAsAdminItem.Click += (_, _) => _applicationManager.SetRunAsAdministrator(entry, !entry.RunAsAdministrator);
        menu.Items.Add(runAsAdminItem);
        menu.IsOpen = true;
        e.Handled = true;
    }

    /// <summary>
    /// Lets any app added from All Apps have its icon changed — a normal exe or a
    /// web app alike, since <see cref="ApplicationManager.SetCustomIcon"/> is keyed
    /// on the entry's own Id rather than anything derived from its LaunchKind. Uses
    /// the same WinForms OpenFileDialog (not WPF's Microsoft.Win32 one) as the
    /// category icon picker in CategoryManagementWindow.xaml.cs — that swap was a
    /// deliberate fix for a broken-shell-component failure in the WPF dialog on at
    /// least one user's machine, so any new file picker needs to reuse it too.
    /// </summary>
    private void ChangeAppIcon(ApplicationEntry entry)
    {
        try
        {
            using var dialog = new System.Windows.Forms.OpenFileDialog
            {
                Title = "Choose an icon",
                Filter = "Image files (*.png;*.ico;*.svg)|*.png;*.ico;*.svg",
                AutoUpgradeEnabled = false,
            };

            var ownerHandle = new System.Windows.Interop.WindowInteropHelper(this).Handle;
            if (dialog.ShowDialog(new Win32WindowHandle(ownerHandle)) != System.Windows.Forms.DialogResult.OK)
                return;

            _applicationManager.SetCustomIcon(entry, dialog.FileName);
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, $"Couldn't use that file as an icon: {ex.Message}", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    /// <summary>
    /// "Move to Category" submenu (round-4 feedback: "cannot add apps into parent
    /// category from the All menu — how to add apps into that category?"). The
    /// only prior way was dragging an entry from this same popup onto a category
    /// icon — this adds a discoverable non-drag alternative next to it, using
    /// CategoryManager.GetAllFlattened() (present since an earlier phase, built
    /// for exactly this kind of picker, but never actually wired to any UI until
    /// now) for a depth-indented flat list of every category/subcategory.
    /// </summary>
    private MenuItem BuildMoveToCategoryMenuItem(ApplicationEntry entry)
    {
        var moveItem = new MenuItem { Header = "Move to Category" };

        var uncategorizedItem = new MenuItem { Header = "Uncategorized", IsCheckable = true, IsChecked = entry.CategoryId is null };
        uncategorizedItem.Click += (_, _) => _applicationManager.SetCategory(entry, null);
        moveItem.Items.Add(uncategorizedItem);

        var categories = _categoryManager.GetAllFlattened().ToList();
        if (categories.Count > 0)
            moveItem.Items.Add(new Separator());

        foreach (var (category, depth) in categories)
        {
            var categoryItem = new MenuItem
            {
                Header = new string(' ', depth * 3) + category.Name,
                IsCheckable = true,
                IsChecked = entry.CategoryId == category.Id,
            };
            categoryItem.Click += (_, _) => _applicationManager.SetCategory(entry, category.Id);
            moveItem.Items.Add(categoryItem);
        }

        return moveItem;
    }

    private static ListBoxItem? FindAncestorListBoxItem(DependencyObject? source)
    {
        while (source is not null && source is not ListBoxItem)
            source = VisualTreeHelper.GetParent(source);

        return source as ListBoxItem;
    }

    /// <summary>
    /// Full rebuild of the center panel on every change (category or pin/unpin
    /// changes). Simpler and less error-prone than incremental diffing, and cheap
    /// enough for the app/category counts a launcher actually deals with — see
    /// DEVELOPMENT.md for the note on revisiting this if it ever shows up as a real
    /// cost.
    ///
    /// Renders only root category icons and pinned app shortcuts (round feedback:
    /// "remove opened apps display on the taskbar, remain only apps category and
    /// apps shortcut" — this app now works purely as a launcher, not a window
    /// switcher, so there's no more running-window tracking at all).
    ///
    /// The two are merged into one row ordered by SortOrder rather than rendered
    /// as two separate category-then-app blocks (round feedback: "make also the
    /// category can be reorder on the right with apps shortcut link") — categories
    /// and pinned apps share the same ordering scale (see
    /// <see cref="ReorderCenterPanelItem"/>) so either can be dragged to sit
    /// anywhere among the other. OrderBy is stable, so untouched data (identical
    /// SortOrder values from the two types' independent "append at the end"
    /// numbering) still falls back to today's categories-before-apps grouping —
    /// only an explicit drag actually interleaves them.
    ///
    /// Deferred to a later dispatcher pass rather than rebuilding synchronously —
    /// this method is subscribed to CategoryManager/ApplicationManager's Changed
    /// event, which a center-panel reorder fires *from inside* the still-active
    /// DragDrop.DoDragDrop call for whichever button is being dragged (see
    /// AttachDragSource). Clearing CenterPanel.Children synchronously at that
    /// point tears the drag source out of the visual tree while WPF's own
    /// drag-drop machinery is still mid-operation on it — round feedback: a
    /// two-step reorder test where the second step ("drag category1 to the far
    /// right") produced an unrelated category jumping to the front, consistent
    /// with the *previous* reorder having only half-applied (categories renumbered,
    /// apps not) after an aborted rebuild. Running once the current dispatcher
    /// frame (and DoDragDrop with it) has finished avoids that entirely.
    /// </summary>
    private void RefreshCenterPanel() => Dispatcher.BeginInvoke(RefreshCenterPanelCore, DispatcherPriority.Background);

    private void RefreshCenterPanelCore()
    {
        CenterPanel.Children.Clear();

        foreach (var item in GetCenterPanelItems())
        {
            CenterPanel.Children.Add(item switch
            {
                ApplicationCategory category => BuildCategoryButton(category),
                ApplicationEntry app => BuildPinnedAppButton(app),
                _ => throw new InvalidOperationException($"Unexpected center panel item type: {item.GetType()}"),
            });
        }
    }

    /// <summary>
    /// The single source of truth for center-panel order — root categories and
    /// pinned apps merged and sorted by their shared SortOrder scale. Both
    /// <see cref="RefreshCenterPanelCore"/> (what's rendered) and
    /// <see cref="ReorderCenterPanelItem"/> (what a drop's insertion index is
    /// spliced into) must use this exact same sequence, or an index computed
    /// against the rendered buttons (<see cref="ComputeInsertionIndex"/>) ends up
    /// applied to a differently-ordered list — which is exactly what was
    /// happening: <see cref="ReorderCenterPanelItem"/> used to concatenate
    /// GetRootCategories() and GetPinnedApplications() *without* the final
    /// interleaving sort applied here, so the moment the two were ever
    /// interleaved (which the very first reorder always does), the index the UI
    /// showed and the index the splice used stopped matching, and drags started
    /// producing seemingly-random results (round feedback: dragging one category
    /// moved an unrelated one instead).
    /// </summary>
    private List<object> GetCenterPanelItems() =>
        _categoryManager.GetRootCategories().Cast<object>()
            .Concat(_applicationManager.GetPinnedApplications().Cast<object>())
            .OrderBy(GetCenterPanelSortOrder)
            .ThenBy(GetCenterPanelTypeRank) // deterministic tie-break — see below
            .ToList();

    private static int GetCenterPanelSortOrder(object item) => item switch
    {
        ApplicationCategory category => category.SortOrder,
        ApplicationEntry app => app.SortOrder,
        _ => 0,
    };

    /// <summary>
    /// Categories and pinned apps are numbered independently (a freshly-created
    /// root category and a freshly-pinned app can easily land on the same
    /// SortOrder value, e.g. both "0" before anything's ever been explicitly
    /// reordered) — an explicit tie-break, rather than leaning on OrderBy's
    /// stability plus whatever order Concat happened to produce, is what actually
    /// guarantees "categories before apps" for untouched data. Once
    /// <see cref="ReorderCenterPanelItem"/> renumbers a set of items, their
    /// SortOrder values are unique and this tie-break never applies to them again.
    /// </summary>
    private static int GetCenterPanelTypeRank(object item) => item is ApplicationCategory ? 0 : 1;

    /// <summary>
    /// Authoritative drop zone for center-panel reordering (round feedback: "i
    /// still cannot drag and drop category to the right side") — accepts a
    /// category or an already-pinned app being dragged (anything else, e.g. an
    /// unpinned entry meant for categorizing/pinning, is left for the individual
    /// button handlers, which run first since they're deeper in the visual tree
    /// and this only sees events they didn't claim). Reacts to the whole panel,
    /// not just individual buttons, so a drop anywhere along the row — including
    /// past the last item — resolves to a real position instead of silently doing
    /// nothing.
    /// </summary>
    private void CenterPanel_DragOver(object sender, DragEventArgs e)
    {
        if (!IsCenterPanelReorderPayload(e.Data))
        {
            RemoveReorderIndicator();
            return;
        }

        e.Effects = DragDropEffects.Move;
        ShowReorderIndicator(ComputeInsertionIndex(e.GetPosition(CenterPanel).X));
        e.Handled = true;
    }

    private void CenterPanel_DragLeave(object sender, DragEventArgs e) => RemoveReorderIndicator();

    /// <summary>
    /// Position-based drop (round feedback: "make a indicator that show where
    /// will the apps shortcut and category will end up after drag and drop") —
    /// resolves to an insertion index from the drop's X position rather than
    /// requiring the drop to land exactly on another button, which is what made
    /// reordering feel broken in practice (see <see cref="CategoryButton_DragOver"/>'s
    /// doc comment).
    /// </summary>
    private void CenterPanel_Drop(object sender, DragEventArgs e)
    {
        if (!IsCenterPanelReorderPayload(e.Data))
            return;

        var insertionIndex = ComputeInsertionIndex(e.GetPosition(CenterPanel).X);
        RemoveReorderIndicator();

        if (e.Data.GetData(typeof(ApplicationCategory)) is ApplicationCategory draggedCategory)
            ReorderCenterPanelItem(draggedCategory, insertionIndex);
        else if (e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry draggedApp)
            ReorderCenterPanelItem(draggedApp, insertionIndex);

        e.Handled = true;
    }

    private static bool IsCenterPanelReorderPayload(IDataObject data) =>
        data.GetDataPresent(typeof(ApplicationCategory)) ||
        (data.GetDataPresent(typeof(ApplicationEntry)) && data.GetData(typeof(ApplicationEntry)) is ApplicationEntry { IsPinned: true });

    /// <summary>Index (into the same order <see cref="RefreshCenterPanel"/> builds)
    /// the dragged item would land at if dropped at <paramref name="mouseX"/> —
    /// before the first button whose horizontal center is past the cursor, or at
    /// the end if the cursor is past every button (including empty space to the
    /// right of the last one, which previously wasn't a valid drop target at
    /// all).</summary>
    private int ComputeInsertionIndex(double mouseX)
    {
        var buttons = CenterPanel.Children.OfType<Button>().ToList();
        for (var i = 0; i < buttons.Count; i++)
        {
            var center = buttons[i].TranslatePoint(new Point(buttons[i].ActualWidth / 2, 0), CenterPanel).X;
            if (mouseX < center)
                return i;
        }

        return buttons.Count;
    }

    /// <summary>Thin vertical bar positioned in <see cref="CenterPanelOverlay"/> — a
    /// floating element on top of <see cref="CenterPanel"/>, not a real sibling
    /// inserted into it, so showing/moving the indicator never changes
    /// CenterPanel's own content width. It used to be a real sibling, which
    /// worked for the WrapPanel-based category flyout (see
    /// <see cref="ShowCategoryReorderIndicator"/>) but broke badly here: CenterPanel
    /// is HorizontalAlignment="Center", so every insert/remove changed its total
    /// width and recentered the *entire row* — round feedback: "it just exchange
    /// position with other category, jump to the left and right" (and, since
    /// button positions were shifting out from under <see cref="ComputeInsertionIndex"/>
    /// mid-drag, the wrong final splice — "apps shortcut not persisted on the left
    /// side").</summary>
    private void ShowReorderIndicator(int index)
    {
        _reorderIndicator ??= new Border { Width = 3, Background = (Brush)FindResource("AccentBrush") };

        if (!CenterPanelOverlay.Children.Contains(_reorderIndicator))
            CenterPanelOverlay.Children.Add(_reorderIndicator);

        var xInCenterPanel = GetInsertionXInCenterPanel(index);
        var xInOverlay = CenterPanel.TranslatePoint(new Point(xInCenterPanel, 0), CenterPanelOverlay).X;
        var height = Math.Max(CenterPanel.ActualHeight - 8, 16);

        _reorderIndicator.Height = height;
        Canvas.SetLeft(_reorderIndicator, xInOverlay - _reorderIndicator.Width / 2);
        Canvas.SetTop(_reorderIndicator, (CenterPanelOverlay.ActualHeight - height) / 2);
    }

    /// <summary>X position (in CenterPanel's own coordinate space) the indicator
    /// should sit at for <paramref name="index"/> — the left edge of the button
    /// currently at that index, or the right edge of the last button if
    /// <paramref name="index"/> is past the end (dropping after everything).</summary>
    private double GetInsertionXInCenterPanel(int index)
    {
        var buttons = CenterPanel.Children.OfType<Button>().ToList();
        if (buttons.Count == 0)
            return 0;

        if (index >= buttons.Count)
        {
            var last = buttons[^1];
            return last.TranslatePoint(new Point(last.ActualWidth, 0), CenterPanel).X;
        }

        return buttons[index].TranslatePoint(new Point(0, 0), CenterPanel).X;
    }

    private void RemoveReorderIndicator()
    {
        if (_reorderIndicator is not null)
            CenterPanelOverlay.Children.Remove(_reorderIndicator);
    }

    /// <summary>
    /// Unified center-panel reorder — moves <paramref name="dragged"/> (a root
    /// <see cref="ApplicationCategory"/> or a pinned <see cref="ApplicationEntry"/>)
    /// to sit at <paramref name="insertionIndex"/> in the single merged row
    /// <see cref="RefreshCenterPanel"/> renders. Renumbers the *entire* combined
    /// sequence together — never just one type at a time — since that's what
    /// makes interleaving stick: renumbering only, say, the categories back to a
    /// compact 0..k-1 range would silently undo any earlier cross-type interleave
    /// the next time two categories were reordered among themselves (round
    /// feedback: "make also the category can be reorder on the right with apps
    /// shortcut link").
    /// </summary>
    private void ReorderCenterPanelItem(object dragged, int insertionIndex)
    {
        var combined = GetCenterPanelItems();

        var draggedIndex = combined.FindIndex(x => ReferenceEquals(x, dragged));
        if (draggedIndex < 0)
            return;

        combined.RemoveAt(draggedIndex);
        if (draggedIndex < insertionIndex)
            insertionIndex--; // removing an earlier item shifts everything after it left by one

        combined.Insert(Math.Clamp(insertionIndex, 0, combined.Count), dragged);

        var categoryOrder = new Dictionary<string, int>();
        var appOrder = new Dictionary<string, int>();
        for (var i = 0; i < combined.Count; i++)
        {
            switch (combined[i])
            {
                case ApplicationCategory category:
                    categoryOrder[category.Id] = i;
                    break;
                case ApplicationEntry app:
                    appOrder[app.Id] = i;
                    break;
            }
        }

        _categoryManager.ApplyCenterPanelOrder(categoryOrder);
        _applicationManager.ApplyCenterPanelOrder(appOrder);
    }

    /// <summary>Pinned app shortcut — clicking launches it. Purely a launcher entry
    /// now (round feedback: "act like an app launcher"); there's no running/not-running
    /// distinction to render since the taskbar no longer tracks open windows.</summary>
    private Button BuildPinnedAppButton(ApplicationEntry entry)
    {
        var button = new Button
        {
            Content = BuildIconContent(entry),
            Style = (Style)FindResource("TaskbarButtonStyle"),
            Tag = entry,
            AllowDrop = true,
            ToolTip = entry.Name,
        };
        AutomationProperties.SetName(button, entry.Name);
        button.Click += (_, _) => _applicationManager.Launch(entry);
        button.ContextMenu = BuildPinnedAppContextMenu(entry);
        button.DragEnter += PinnedAppButton_DragOver;
        button.DragOver += PinnedAppButton_DragOver;
        button.Drop += PinnedAppButton_Drop;
        AttachUnpinDragSource(button, entry);

        return button;
    }

    /// <summary>
    /// Only claims "pin an app that isn't pinned yet" (e.g. dragged in from the
    /// All Applications list) — unambiguous, target-specific. A category being
    /// dragged, or an already-pinned app being dragged (both mean "reorder"), is
    /// deliberately left unhandled so the event bubbles up to
    /// <see cref="CenterPanel_DragOver"/> — see <see cref="CategoryButton_DragOver"/>'s
    /// doc comment for why button-level handling of reorder drags was the actual
    /// bug behind "i still cannot drag and drop category to the right side".
    /// </summary>
    private void PinnedAppButton_DragOver(object sender, DragEventArgs e)
    {
        if (e.Data.GetDataPresent(typeof(ApplicationEntry)) &&
            e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry { IsPinned: false })
        {
            e.Effects = DragDropEffects.Move;
            e.Handled = true;
        }
    }

    private void PinnedAppButton_Drop(object sender, DragEventArgs e)
    {
        if (e.Data.GetData(typeof(ApplicationEntry)) is ApplicationEntry { IsPinned: false } dragged)
        {
            _applicationManager.SetPinned(dragged, true);
            e.Handled = true; // don't let this bubble into Taskbar_Drop and double-pin
        }
        // Already-pinned entries and categories: deliberately not handled — bubbles to CenterPanel_Drop.
    }

    private ContextMenu BuildPinnedAppContextMenu(ApplicationEntry entry)
    {
        var menu = new ContextMenu();
        AddMenuAction(menu, "Open", () => _applicationManager.Launch(entry));
        AddMenuAction(menu, "Unpin from taskbar", () => _applicationManager.SetPinned(entry, false));
        menu.Items.Add(new Separator());
        menu.Items.Add(BuildMoveToCategoryMenuItem(entry));
        return menu;
    }

    /// <summary>
    /// Renders a pinned button's icon straight from its own
    /// <see cref="ApplicationEntry"/>, so a user-picked custom icon
    /// (<see cref="ApplicationEntry.CustomIconPath"/>) is honored correctly.
    ///
    /// Used to instead re-resolve the entry by matching
    /// <see cref="ApplicationEntry.ExecutablePath"/> against every known app —
    /// redundant work, since the caller already had the exact entry in hand, and
    /// actively wrong whenever two entries share the same target executable with
    /// different <see cref="ApplicationEntry.Arguments"/> (e.g. several
    /// mmc.exe-based admin tools — Services, Event Viewer, Disk Management —
    /// which all resolve to the same mmc.exe path but launch different .msc
    /// snap-ins): FirstOrDefault would grab whichever of those entries happened
    /// to come first, not necessarily the one actually being rendered, so a
    /// custom icon set on one could silently show up on another instead (or not
    /// show up at all, if a same-exe sibling without a custom icon won the
    /// lookup) — reported live as "why it show windows mmc icon?" after setting
    /// a custom icon on a manually-added Services shortcut.
    /// </summary>
    private Image? BuildIconContent(ApplicationEntry entry)
    {
        var icon = ApplicationIconConverter.IconService.GetIcon(entry);
        // Was 22x22 — round feedback, with a side-by-side screenshot: FlexTaskbar's
        // icons read as noticeably smaller than the real Windows 11 taskbar's at
        // the same bar height, swimming in extra padding, which is what actually
        // looked "not sharp" — every earlier fix this session (icon cache
        // resolution, native extraction resolution, layout rounding) addressed
        // genuine pixel-level softness, but none of them touched render *size*,
        // which turned out to be the actual visible difference. 32px is much
        // closer to how large native taskbar icons sit within the same ~48px bar
        // height (_configuredHeight's default).
        return icon is null ? null : new Image { Width = 32, Height = 32, Margin = new Thickness(4, 0, 4, 0), Source = icon };
    }

    /// <summary>
    /// Renders a category button using the same icon-first visual language as the
    /// running-window buttons (spec request: "blend the category parent apps with
    /// the existing apps icon on the taskbar") instead of the previous
    /// "📁 Name ▾" text label. Falls back to the category's emoji glyph (or a
    /// generic folder icon) when no custom image icon has been set — see
    /// <see cref="ApplicationCategory.CustomIconPath"/>.
    /// </summary>
    private static UIElement BuildCategoryIconContent(ApplicationCategory category)
    {
        // Was 22x22 — see BuildIconContent's comment; same fix, same reasoning.
        var grid = new Grid { Width = 32, Height = 32, Margin = new Thickness(4, 0, 4, 0) };

        if (category.CustomIconPath is { } path && CategoryIconService.TryLoadIcon(path, out var bitmap))
        {
            grid.Children.Add(new Image { Width = 32, Height = 32, Source = bitmap });
        }
        else
        {
            grid.Children.Add(new TextBlock
            {
                Text = string.IsNullOrEmpty(category.IconGlyph) ? "📁" : category.IconGlyph,
                FontSize = 22, // was 16 — scaled up along with the grid, above
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            });
        }

        grid.Children.Add(new TextBlock
        {
            Text = "▾",
            FontSize = 8,
            Foreground = (Brush)Application.Current.FindResource("TaskbarSubtleForegroundBrush"),
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Bottom,
        });

        return grid;
    }

    private static void AddMenuAction(ContextMenu menu, string header, Action action)
    {
        var item = new MenuItem { Header = header };
        item.Click += (_, _) => action();
        menu.Items.Add(item);
    }

    private static string Truncate(string text, int maxLength) =>
        text.Length <= maxLength ? text : text[..(maxLength - 1)] + "…";

    // --- Phase 6: auto-hide + AppBar screen-space reservation ---
    // Both real, both off by default, both reachable only via this right-click menu
    // for now — a proper toggle belongs in Settings (Phase 7), but per the "No Fake
    // Features" rule these needed *some* honest way to reach them now that they exist.

    private void Background_MouseRightButtonUp(object sender, MouseButtonEventArgs e)
    {
        var menu = new ContextMenu
        {
            PlacementTarget = (UIElement)sender,
            Placement = PlacementMode.MousePoint,
        };

        var autoHideItem = new MenuItem { Header = "Auto-hide taskbar", IsCheckable = true, IsChecked = _autoHideEnabled };
        autoHideItem.Click += (_, _) => ApplyAutoHideSetting(!_autoHideEnabled);
        menu.Items.Add(autoHideItem);

        var reserveItem = new MenuItem
        {
            Header = "Reserve screen space (AppBar)",
            IsCheckable = true,
            IsChecked = _layoutManager?.IsRegistered ?? false,
        };
        reserveItem.Click += (_, _) => ApplyReserveScreenSpaceSetting(!IsReservingScreenSpace);
        menu.Items.Add(reserveItem);

        var newCategoryItem = new MenuItem { Header = "New Category..." };
        newCategoryItem.Click += (_, _) => NewCategoryButton_Click(sender, e);
        menu.Items.Add(new Separator());
        menu.Items.Add(newCategoryItem);

        var settingsItem = new MenuItem { Header = "Settings..." };
        settingsItem.Click += (_, _) => OpenSettings();
        var recoveryItem = new MenuItem { Header = "Recovery..." };
        recoveryItem.Click += (_, _) => OpenRecovery();
        menu.Items.Add(new Separator());
        menu.Items.Add(settingsItem);
        menu.Items.Add(recoveryItem);

        menu.IsOpen = true;
    }

    private void SetAutoHide(bool enabled)
    {
        _autoHideEnabled = enabled;
        _autoHideTimer.Stop();

        if (!enabled)
            Expand();
        else if (!IsMouseOver)
            Collapse();
    }

    private void ToggleReserveScreenSpace()
    {
        if (_layoutManager is null)
            return;

        if (_layoutManager.IsRegistered)
        {
            _layoutManager.Unregister();
            ApplyDockedPositionNonAppBar(_isExpanded ? _configuredHeight : CollapsedHeight);
        }
        else
        {
            _layoutManager.BarThickness = _isExpanded ? _configuredHeight : CollapsedHeight;
            _layoutManager.Register();
        }
    }

    private void TaskbarWindow_MouseEnter(object sender, MouseEventArgs e)
    {
        _autoHideTimer.Stop();
        if (_autoHideEnabled)
            Expand();
    }

    private void TaskbarWindow_MouseLeave(object sender, MouseEventArgs e)
    {
        if (_autoHideEnabled)
        {
            _autoHideTimer.Stop();
            _autoHideTimer.Start();
        }
    }

    private void Expand()
    {
        if (_isExpanded)
            return;

        _isExpanded = true;
        ApplyHeight(_configuredHeight);
    }

    private void Collapse()
    {
        if (!_isExpanded)
            return;

        _isExpanded = false;
        ApplyHeight(CollapsedHeight);
    }

    private void ApplyHeight(double height)
    {
        if (_layoutManager is { IsRegistered: true })
        {
            _layoutManager.BarThickness = height;
            _layoutManager.ApplyPosition();
        }
        else
        {
            ApplyDockedPositionNonAppBar(height);
        }
    }

    /// <summary>Direct (non-AppBar) window positioning, respecting <see cref="_position"/>.
    /// The one place that knows how to compute Top for both Bottom- and Top-docked
    /// taskbars without reserving any work area — used by the constructor and
    /// whenever AppBar mode is off.</summary>
    private void ApplyDockedPositionNonAppBar(double height)
    {
        Height = height;
        Left = SystemParameters.WorkArea.Left;
        Width = SystemParameters.WorkArea.Width;
        Top = _position == TaskbarPosition.Top ? SystemParameters.WorkArea.Top : SystemParameters.WorkArea.Bottom - height;
    }

    /// <summary>
    /// Excludes FlexTaskbar itself from Alt-Tab switching (round feedback: "make
    /// the flextaskbar exclude form Alt-Tab apps switching") — the real Windows
    /// taskbar never shows up as an Alt-Tab candidate either. WPF's
    /// ShowInTaskbar="False" (already set in XAML) adds WS_EX_TOOLWINDOW on its
    /// own, which is usually enough on its own to drop a window from Alt-Tab, but
    /// evidently wasn't reliably keeping FlexTaskbar out here — asserting it
    /// directly (and clearing WS_EX_APPWINDOW, which forces a window back into
    /// Alt-Tab/the taskbar even with WS_EX_TOOLWINDOW set) removes any ambiguity.
    /// Idempotent, so it's safe to call every time the window's shown.
    /// </summary>
    private void ExcludeFromAltTab()
    {
        var hwnd = new WindowInteropHelper(this).Handle;
        if (hwnd == IntPtr.Zero)
            return;

        var exStyle = User32.GetWindowLongPtr(hwnd, User32.GWL_EXSTYLE).ToInt64();
        exStyle = (exStyle | User32.WS_EX_TOOLWINDOW) & ~User32.WS_EX_APPWINDOW;
        User32.SetWindowLongPtr(hwnd, User32.GWL_EXSTYLE, new IntPtr(exStyle));
    }

    /// <summary>Re-pins the taskbar to the topmost z-order band without stealing
    /// focus. See the constructor's _topmostTimer comment for why this needs to be
    /// reasserted periodically rather than set once.
    ///
    /// Skipped whenever a real Windows shell popup — Start menu, the volume/
    /// network/battery flyouts, "show hidden icons", Quick Settings, the tray's
    /// own right-click menu, a taskbar icon's hover thumbnail preview, etc. — is
    /// currently showing (round feedback: "make the windows 11 system tray right
    /// click menu on top of the flextaskbar", then "flextaskbar still on the top
    /// of the windows taskbar popup menu", then "flextaskbar still on the top of
    /// windows hover opened apps" — each one covering a case the previous fix
    /// missed, including a first attempt at this exact hover-preview case that
    /// used a fixed few-second grace period after the popup appeared: too short
    /// for a hover the user holds for a while, since the popup only fires one
    /// "shown" event, not a repeating one for as long as it stays open — "when
    /// hover windows 11 taskbar apps, the windows still show below flex
    /// taskbar"). Two complementary checks, since no single one catches every
    /// kind of popup:
    /// - <see cref="IsShellPopupForeground"/>: the foreground window belongs to
    ///   explorer.exe or dwm.exe. Catches anything that takes focus — Start menu,
    ///   flyouts, the tray's own menu.
    /// - <see cref="_visibleShellPopups"/>: explorer.exe- or dwm.exe-owned windows
    ///   currently tracked as shown-but-not-yet-hidden, via
    ///   <see cref="OnShellPopupEvent"/>. Needed because hover-triggered popups — like the live thumbnail preview
    ///   that appears when hovering an open app's taskbar icon — never take focus
    ///   at all, so GetForegroundWindow() alone never sees them; tracking actual
    ///   visibility (not a timer) is what lets suppression last exactly as long
    ///   as the user keeps hovering, however long that is.
    /// All of these are hosted by explorer.exe, which is a simple, robust stand-in
    /// for enumerating every popup's window class by name (fragile across Windows
    /// versions — Windows 11's shell flyouts are XAML islands with undocumented
    /// class names). SetWindowPos(HWND_TOPMOST) moves a window to the very top of
    /// the topmost band, so reasserting while one of these was open would cover
    /// it right back up — this is what made them intermittently disappear behind
    /// FlexTaskbar.
    /// </summary>
    private void ReassertTopmost()
    {
        PruneStaleShellPopups();
        if (IsShellPopupForeground() || _visibleShellPopups.Count > 0)
            return;

        var hwnd = new WindowInteropHelper(this).Handle;
        if (hwnd != IntPtr.Zero)
        {
            User32.SetWindowPos(hwnd, User32.HWND_TOPMOST, 0, 0, 0, 0,
                User32.SWP_NOMOVE | User32.SWP_NOSIZE | User32.SWP_NOACTIVATE);
        }
    }

    /// <summary>Safety net for missed HIDE/DESTROY events (WinEvent delivery
    /// isn't guaranteed) — without this, a single dropped event would leave a
    /// stale entry in <see cref="_visibleShellPopups"/> that permanently blocks
    /// reassertion. Cheap enough to run on every tick: this set is small (a
    /// handful of entries at most) and IsWindow is a lightweight call.</summary>
    private void PruneStaleShellPopups() => _visibleShellPopups.RemoveWhere(hwnd => !User32.IsWindowVisible(hwnd));

    private static bool IsShellPopupForeground() => IsShellOwned(User32.GetForegroundWindow());

    /// <summary>Explorer.exe hosts most shell chrome (Start menu, flyouts, the
    /// tray's own menu), but taskbar hover thumbnail *previews* specifically are
    /// composited by dwm.exe (Desktop Window Manager) — recognizing only
    /// "explorer" was still letting those slip through the topmost fight (round
    /// feedback, repeated: "windows still show below flex taskbar" even after
    /// tracking explorer.exe SHOW/HIDE events).</summary>
    private static bool IsShellOwned(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero)
            return false;

        User32.GetWindowThreadProcessId(hwnd, out var processId);
        if (processId == 0)
            return false;

        try
        {
            using var process = Process.GetProcessById((int)processId);
            return string.Equals(process.ProcessName, "explorer", StringComparison.OrdinalIgnoreCase)
                || string.Equals(process.ProcessName, "dwm", StringComparison.OrdinalIgnoreCase);
        }
        catch (ArgumentException)
        {
            return false; // process already exited between the two calls — treat as "not a shell popup"
        }
    }

    /// <summary>
    /// Watches for explorer.exe showing or foregrounding any window at all, and
    /// on every such event, extends a short grace period during which
    /// <see cref="ReassertTopmost"/> stays quiet — the only way to catch
    /// non-activating shell popups (hover thumbnail previews above all) since
    /// they never become the foreground window. The delegate instance is kept in
    /// a field for the lifetime of the hook — the CLR has no other reference to
    /// it, so letting it get GC'd would silently break the hook (classic
    /// P/Invoke pitfall, same reasoning as User32.WinEventDelegate's own doc
    /// comment).
    /// </summary>
    private void InitializeShellPopupWatcher()
    {
        _shellPopupEventCallback = OnShellPopupEvent;
        _shellForegroundHook = User32.SetWinEventHook(
            User32.EVENT_SYSTEM_FOREGROUND, User32.EVENT_SYSTEM_FOREGROUND,
            IntPtr.Zero, _shellPopupEventCallback, 0, 0,
            User32.WINEVENT_OUTOFCONTEXT | User32.WINEVENT_SKIPOWNPROCESS);

        // EVENT_OBJECT_DESTROY/SHOW/HIDE are contiguous (0x8001-0x8003), so one
        // hook covers all three — DESTROY is included as a second safety net
        // alongside PruneStaleShellPopups for popups that get torn down without
        // ever firing HIDE.
        _shellObjectHook = User32.SetWinEventHook(
            User32.EVENT_OBJECT_DESTROY, User32.EVENT_OBJECT_HIDE,
            IntPtr.Zero, _shellPopupEventCallback, 0, 0,
            User32.WINEVENT_OUTOFCONTEXT | User32.WINEVENT_SKIPOWNPROCESS);
    }

    /// <summary>
    /// Deliberately does *not* filter on idObject (OBJID_WINDOW) — a hover
    /// thumbnail preview's outer frame may report as a non-window object
    /// (OBJID_CLIENT or otherwise) depending on how it's composited, and
    /// filtering on that turned out to be dropping the very event this exists to
    /// catch. Any SHOW/HIDE/DESTROY for an explorer.exe- or dwm.exe-owned hwnd is
    /// tracked; hwnd is what actually goes in <see cref="_visibleShellPopups"/>,
    /// so a child-object notification just adds/removes the same parent window
    /// handle a window-level one would, which is harmless (HashSet operations are
    /// idempotent).
    /// </summary>
    private void OnShellPopupEvent(IntPtr hWinEventHook, uint eventType, IntPtr hwnd, int idObject, int idChild, uint dwEventThread, uint dwmsEventTime)
    {
        if (hwnd == IntPtr.Zero)
            return;

        switch (eventType)
        {
            case User32.EVENT_OBJECT_SHOW when IsShellOwned(hwnd):
                _visibleShellPopups.Add(hwnd);
                break;
            case User32.EVENT_OBJECT_HIDE:
            case User32.EVENT_OBJECT_DESTROY:
                _visibleShellPopups.Remove(hwnd); // no IsShellOwned check needed — Remove on an untracked hwnd is a harmless no-op
                break;
        }
    }
}
