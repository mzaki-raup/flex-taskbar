using System.Diagnostics;
using System.Windows;
using FlexTaskbar.Services;
using FlexTaskbar.Settings;
using FlexTaskbar.Taskbar;

namespace FlexTaskbar.Views;

/// <summary>
/// The emergency recovery surface (spec Section 30), opened either by
/// Ctrl+Alt+Shift+F12 from a normally-running <see cref="TaskbarWindow"/>, or as the
/// *only* window <c>FlexTaskbar.exe --safe-mode</c> shows. Deliberately does not
/// require a live TaskbarWindow to function — every action here either operates
/// directly on persisted config/OS state (works even if the taskbar is completely
/// broken) or, when a live taskbar instance is available, also applies immediately
/// to it. This is what "recovery must work even if the normal taskbar is
/// unavailable" (Section 30) actually requires architecturally, not just as a
/// stated goal.
/// </summary>
public partial class RecoveryWindow : Window
{
    private readonly TaskbarWindow? _taskbarWindow;
    private readonly bool _isSafeMode;

    public RecoveryWindow(TaskbarWindow? taskbarWindow, bool isSafeMode)
    {
        InitializeComponent();
        _taskbarWindow = taskbarWindow;
        _isSafeMode = isSafeMode;

        SubtitleText.Text = isSafeMode
            ? "Running in Safe Mode — the taskbar itself is disabled. These actions operate on saved configuration and the OS directly."
            : "Ctrl+Alt+Shift+F12 recovery menu — these actions work even if the taskbar is misbehaving.";

        OpenSettingsButton.IsEnabled = _taskbarWindow is not null;
        OpenSettingsButton.ToolTip = _taskbarWindow is null
            ? "Settings requires the taskbar to be running normally — not available in Safe Mode."
            : null;
    }

    private void RestoreTaskbar_Click(object sender, RoutedEventArgs e)
    {
        var settingsManager = new SettingsManager();
        settingsManager.Load();
        settingsManager.Current.ReserveScreenSpace = false;
        settingsManager.Current.AutoHide = false;
        settingsManager.Save();

        // Also fix the *live* instance immediately, if there is one, rather than
        // only the file a future launch would read.
        _taskbarWindow?.ApplyReserveScreenSpaceSetting(false);
        _taskbarWindow?.ApplyAutoHideSetting(false);

        SetStatus("Screen-space reservation and auto-hide disabled (saved, and applied immediately if the taskbar is running).");
    }

    private void RestartExplorer_Click(object sender, RoutedEventArgs e)
    {
        var result = MessageBox.Show(
            this,
            "This will restart Windows Explorer (explorer.exe). Your desktop and real taskbar will briefly disappear and reload. Continue?",
            "Restart Explorer",
            MessageBoxButton.YesNo,
            MessageBoxImage.Warning);

        if (result != MessageBoxResult.Yes)
            return;

        try
        {
            foreach (var process in Process.GetProcessesByName("explorer"))
            {
                process.Kill();
                process.WaitForExit(5000);
            }

            Process.Start(new ProcessStartInfo { FileName = "explorer.exe", UseShellExecute = true });
            SetStatus("Explorer restarted.");
        }
        catch (Exception ex) when (ex is InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            SetStatus("Could not restart Explorer — you may need to do this manually via Task Manager.");
        }
    }

    private void DisableStartup_Click(object sender, RoutedEventArgs e)
    {
        StartupService.SetEnabled(false);
        SetStatus("FlexTaskbar auto-start disabled.");
    }

    private void OpenSettings_Click(object sender, RoutedEventArgs e)
    {
        _taskbarWindow?.OpenSettings();
    }

    private void SafeMode_Click(object sender, RoutedEventArgs e)
    {
        if (_isSafeMode)
        {
            SetStatus("Already running in Safe Mode.");
            return;
        }

        try
        {
            var exePath = Process.GetCurrentProcess().MainModule?.FileName;
            if (!string.IsNullOrEmpty(exePath))
                Process.Start(new ProcessStartInfo { FileName = exePath, Arguments = "--safe-mode", UseShellExecute = true });

            Application.Current.Shutdown();
        }
        catch (System.ComponentModel.Win32Exception)
        {
            SetStatus("Could not launch Safe Mode — try running FlexTaskbar.exe --safe-mode manually.");
        }
    }

    private void SetStatus(string message)
    {
        StatusText.Text = message;
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();
}
