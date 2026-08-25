using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using FlexTaskbar.Applications;
using FlexTaskbar.Services;
using FlexTaskbar.Taskbar;
using FlexTaskbar.Utilities;
using Microsoft.Win32;

namespace FlexTaskbar.Settings;

/// <summary>
/// The real home for preferences that Phases 5–6 had to stick on a temporary
/// right-click menu (auto-hide, AppBar reservation) plus the rest of spec Section
/// 29: startup, appearance, hotkey reassignment, and Section 41's import/export.
/// Deliberately does not have a separate "Categories" tab — category management
/// already has a real, working home (the "⚙ Manage Category" item in each
/// category's own flyout, per-category rather than a big tree editor here.
///
/// All controls are wired to real state on <see cref="TaskbarWindow"/> — nothing
/// here is decorative. Changes apply immediately and persist via
/// <see cref="SettingsManager"/>, there's no separate "Apply"/"OK" step to forget.
/// </summary>
public partial class SettingsWindow : Window
{
    private readonly TaskbarWindow _taskbarWindow;
    private bool _isInitializing = true;

    public SettingsWindow(TaskbarWindow taskbarWindow)
    {
        InitializeComponent();
        _taskbarWindow = taskbarWindow;

        LoadInitialState();
        _isInitializing = false;
    }

    private void LoadInitialState()
    {
        StartWithWindowsCheckBox.IsChecked = StartupService.IsEnabled;
        AutoHideCheckBox.IsChecked = _taskbarWindow.AutoHideEnabled;
        ReserveSpaceCheckBox.IsChecked = _taskbarWindow.IsReservingScreenSpace;

        AppCountText.Text = $"{_taskbarWindow.ApplicationManagerInstance.Applications.Count} applications discovered";

        PositionBottomRadio.IsChecked = _taskbarWindow.CurrentPosition == TaskbarPosition.Bottom;
        PositionTopRadio.IsChecked = _taskbarWindow.CurrentPosition == TaskbarPosition.Top;
        HeightSlider.Value = _taskbarWindow.CurrentHeight;
        HeightValueText.Text = $"{(int)_taskbarWindow.CurrentHeight}px";

        foreach (var name in HotkeyPresets.Names)
            HotkeyComboBox.Items.Add(name);
        HotkeyComboBox.SelectedItem = _taskbarWindow.SettingsManagerInstance.Current.HotkeyPreset;
        UpdateHotkeyStatus();
    }

    private void UpdateHotkeyStatus()
    {
        HotkeyStatusText.Text = "Applies immediately when you change the selection above.";
        HotkeyStatusText.Foreground = (System.Windows.Media.Brush)FindResource("TaskbarSubtleForegroundBrush");
    }

    private void StartWithWindowsCheckBox_Changed(object sender, RoutedEventArgs e)
    {
        if (_isInitializing) return;
        StartupService.SetEnabled(StartWithWindowsCheckBox.IsChecked == true);
    }

    private void AutoHideCheckBox_Changed(object sender, RoutedEventArgs e)
    {
        if (_isInitializing) return;
        _taskbarWindow.ApplyAutoHideSetting(AutoHideCheckBox.IsChecked == true);
    }

    private void ReserveSpaceCheckBox_Changed(object sender, RoutedEventArgs e)
    {
        if (_isInitializing) return;
        _taskbarWindow.ApplyReserveScreenSpaceSetting(ReserveSpaceCheckBox.IsChecked == true);
    }

    private async void RescanButton_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            await _taskbarWindow.ApplicationManagerInstance.RescanAsync();
            AppCountText.Text = $"{_taskbarWindow.ApplicationManagerInstance.Applications.Count} applications discovered";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            AppCountText.Text = "Rescan failed — see previous count.";
        }
    }

    private void PositionRadio_Changed(object sender, RoutedEventArgs e)
    {
        if (_isInitializing) return;
        var position = PositionTopRadio.IsChecked == true ? TaskbarPosition.Top : TaskbarPosition.Bottom;
        _taskbarWindow.ApplyPositionAndHeight(position, HeightSlider.Value);
    }

    private void HeightSlider_ValueChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        // Setting Minimum/Maximum in XAML coerces the Slider's Value and fires this
        // handler *during* InitializeComponent() — before HeightValueText (declared
        // later in the visual tree) has been connected yet. Caught live as a
        // NullReferenceException; the app survived it (App.xaml.cs's
        // DispatcherUnhandledException safety net from Phase 5), but the bug itself
        // is fixed here rather than relied on that net.
        if (HeightValueText is null)
            return;

        HeightValueText.Text = $"{(int)HeightSlider.Value}px";
        if (_isInitializing) return;

        var position = PositionTopRadio.IsChecked == true ? TaskbarPosition.Top : TaskbarPosition.Bottom;
        _taskbarWindow.ApplyPositionAndHeight(position, HeightSlider.Value);
    }

    private void HotkeyComboBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_isInitializing || HotkeyComboBox.SelectedItem is not string preset)
            return;

        var success = _taskbarWindow.ApplyHotkeyPreset(preset);
        _taskbarWindow.SettingsManagerInstance.Current.HotkeyPreset = preset;
        _taskbarWindow.SettingsManagerInstance.Save();

        HotkeyStatusText.Text = success
            ? $"'{preset}' registered successfully."
            : $"'{preset}' could not be registered — it's likely already claimed by Windows or another app. The \"All\" button or a category shortcut on the taskbar still opens apps.";
        HotkeyStatusText.Foreground = success
            ? (System.Windows.Media.Brush)FindResource("TaskbarForegroundBrush")
            : (System.Windows.Media.Brush)FindResource("AccentBrush");
    }

    private void OpenConfigFolder_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            Process.Start(new ProcessStartInfo { FileName = AppPaths.RoamingRoot, UseShellExecute = true });
        }
        catch (System.ComponentModel.Win32Exception)
        {
            // Non-fatal — worst case the user navigates there manually.
        }
    }

    private void Export_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new SaveFileDialog
        {
            FileName = "FlexTaskbarBackup.json",
            Filter = "FlexTaskbar Backup (*.json)|*.json",
        };

        if (dialog.ShowDialog(this) != true)
            return;

        var backup = new BackupData
        {
            Categories = _taskbarWindow.CategoryManagerInstance.Categories.ToList(),
            AppAssignments = _taskbarWindow.ApplicationManagerInstance.GetAssignments().ToList(),
            Settings = _taskbarWindow.SettingsManagerInstance.Current,
        };

        try
        {
            var json = JsonSerializer.Serialize(backup, new JsonSerializerOptions { WriteIndented = true });
            File.WriteAllText(dialog.FileName, json);
            MessageBox.Show(this, "Export complete.", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Information);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            MessageBox.Show(this, "Export failed — check the destination is writable.", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void Import_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog { Filter = "FlexTaskbar Backup (*.json)|*.json" };
        if (dialog.ShowDialog(this) != true)
            return;

        try
        {
            var json = File.ReadAllText(dialog.FileName);
            var backup = JsonSerializer.Deserialize<BackupData>(json);
            if (backup is null)
                throw new JsonException("Empty or invalid backup file.");

            _taskbarWindow.CategoryManagerInstance.ReplaceAll(backup.Categories);
            _taskbarWindow.ApplicationManagerInstance.ApplyAssignments(backup.AppAssignments);

            if (backup.Settings is not null)
            {
                _taskbarWindow.SettingsManagerInstance.Current.HotkeyPreset = backup.Settings.HotkeyPreset;
                _taskbarWindow.SettingsManagerInstance.Save();
            }

            MessageBox.Show(this, "Import complete.", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Information);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException)
        {
            MessageBox.Show(this, "Import failed — the file may not be a valid FlexTaskbar backup.", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void Reset_Click(object sender, RoutedEventArgs e)
    {
        var result = MessageBox.Show(
            this,
            "Reset all categories and settings to defaults? A timestamped backup of the current files is kept in the config folder first. Discovered applications aren't lost, but any category assignment is cleared — they move back to \"All Applications\".",
            "Reset Configuration",
            MessageBoxButton.YesNo,
            MessageBoxImage.Warning);

        if (result != MessageBoxResult.Yes)
            return;

        ResetService.PerformFullReset();

        // Reload the live in-memory state from the now-reset files, so the running
        // instance reflects the reset immediately rather than only after a restart.
        // UncategorizeAll mirrors what PerformFullReset already did to
        // applications.json on disk — every category is gone, so no app should
        // still be pointing at one — but against the live ApplicationManager
        // instance, since RescanAsync's merge would otherwise copy the
        // still-in-memory (pre-reset) CategoryId right back onto each entry.
        _taskbarWindow.CategoryManagerInstance.Initialize();
        _taskbarWindow.SettingsManagerInstance.Load();
        _taskbarWindow.ApplicationManagerInstance.UncategorizeAll();

        MessageBox.Show(this, "Reset complete. Restart FlexTaskbar for appearance/position changes to fully apply.", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    /// <summary>Lets the custom header (replacing the OS title bar — see the XAML
    /// comment on why) still be used to drag the window, same as a real title bar.</summary>
    private void Header_MouseLeftButtonDown(object sender, System.Windows.Input.MouseButtonEventArgs e)
    {
        if (e.ButtonState == System.Windows.Input.MouseButtonState.Pressed)
            DragMove();
    }
}
