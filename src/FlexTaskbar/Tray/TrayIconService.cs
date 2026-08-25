using System.Drawing;
using System.Windows.Forms;

namespace FlexTaskbar.Tray;

/// <summary>
/// System tray icon (spec Section 14). Built on <see cref="NotifyIcon"/> — a
/// deliberate choice over a hand-rolled Shell_NotifyIcon P/Invoke wrapper.
/// NotifyIcon already solves the parts that are easy to get subtly wrong (icon GDI
/// handle lifetime, the hidden message-only window needed to receive tray click
/// callbacks, taskbar-recreated notifications after Explorer restarts) — reimplementing
/// that by hand would be net-negative for correctness, not a "more native" win. The
/// cost is pulling in System.Windows.Forms (<c>UseWindowsForms</c> in the csproj);
/// documented here per Section 14's "document limitations" guidance.
/// </summary>
public sealed class TrayIconService : IDisposable
{
    private readonly NotifyIcon _notifyIcon;
    private readonly Icon _icon;

    public event Action? OpenSettingsRequested;
    public event Action? RescanRequested;
    public event Action? ExitRequested;

    /// <summary>Raised while this tray icon's own right-click menu is open/closed
    /// (round feedback: "make the windows 11 system tray right click menu on top
    /// of the flextaskbar") — TaskbarWindow pauses its periodic re-assert-topmost
    /// timer for the duration, since that timer calling SetWindowPos(HWND_TOPMOST)
    /// while this (also topmost) menu happened to be open would push FlexTaskbar
    /// back above it, covering the very menu the user just opened.</summary>
    public event Action? ContextMenuOpened;
    public event Action? ContextMenuClosed;

    public TrayIconService()
    {
        var resourceStream = System.Windows.Application.GetResourceStream(
            new Uri("pack://application:,,,/Resources/App.ico"));

        _icon = resourceStream is not null
            ? new Icon(resourceStream.Stream)
            : SystemIcons.Application; // fail-open rather than throw if the resource is ever missing

        var menu = new ContextMenuStrip();
        menu.Items.Add("Settings...", null, (_, _) => OpenSettingsRequested?.Invoke());
        menu.Items.Add("Rescan Applications", null, (_, _) => RescanRequested?.Invoke());
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Exit FlexTaskbar", null, (_, _) => ExitRequested?.Invoke());
        menu.Opening += (_, _) => ContextMenuOpened?.Invoke();
        menu.Closed += (_, _) => ContextMenuClosed?.Invoke();

        _notifyIcon = new NotifyIcon
        {
            Icon = _icon,
            Text = "FlexTaskbar",
            Visible = true,
            ContextMenuStrip = menu,
        };
        _notifyIcon.DoubleClick += (_, _) => OpenSettingsRequested?.Invoke();
    }

    public void Dispose()
    {
        _notifyIcon.Visible = false;
        _notifyIcon.Dispose();
        if (!ReferenceEquals(_icon, SystemIcons.Application))
            _icon.Dispose();
    }
}
