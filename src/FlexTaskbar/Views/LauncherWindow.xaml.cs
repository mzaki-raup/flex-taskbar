using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using FlexTaskbar.Applications;
using FlexTaskbar.Menus;
using FlexTaskbar.Services;

namespace FlexTaskbar.Views;

/// <summary>
/// The Start launcher (spec Section 16): search-first, with Recent/Categories/All
/// Applications sections shown when the search box is empty. Also opened directly by
/// the Win+Space global hotkey (Section 17), in which case it behaves identically —
/// there's no separate "search-only" window, just this one always starting with
/// focus in the search box.
///
/// Settings and Power are both present in the footer per spec Section 16 and both
/// fully real — Settings was briefly a disabled Phase-5 placeholder before
/// SettingsWindow existed (Phase 7), but was never rewired afterward; fixed per
/// direct user report ("in the start link, settings button not working").
/// </summary>
public partial class LauncherWindow : Window
{
    private readonly ApplicationManager _applicationManager;
    private readonly CategoryManager _categoryManager;
    private readonly CategoryMenuBuilder _categoryMenuBuilder;
    private readonly Action _openSettings;
    private bool _isClosing;

    public LauncherWindow(ApplicationManager applicationManager, CategoryManager categoryManager, CategoryMenuBuilder categoryMenuBuilder, Action openSettings)
    {
        InitializeComponent();
        _applicationManager = applicationManager;
        _categoryManager = categoryManager;
        _openSettings = openSettings;
        _categoryMenuBuilder = categoryMenuBuilder;

        AttachLaunchBehavior(ResultsListBox);
        AttachLaunchBehavior(RecentListBox);
        AttachLaunchBehavior(AllAppsListBox);

        Loaded += (_, _) =>
        {
            PopulateDefaultContent();
            SearchBox.Focus();
        };

        // Closing on deactivation matches how the real Start menu / search behave —
        // click anywhere else and it goes away. Guarded by _isClosing: Close() itself
        // can trigger a WM_ACTIVATE deactivation while already inside InternalClose(),
        // and calling Close() again re-entrantly there throws InvalidOperationException
        // ("Cannot ... call Close ... while a Window is closing") — caught live during
        // Phase 5 verification, where it crashed the entire taskbar process (no
        // DispatcherUnhandledException handler existed yet — see App.xaml.cs fix).
        Closing += (_, _) => _isClosing = true;
        Deactivated += (_, _) =>
        {
            if (!_isClosing)
                Close();
        };
    }

    /// <summary>Positions the launcher just above the taskbar, left-aligned to the work area — matches where a real Start menu opens.</summary>
    public void PositionAboveTaskbar(double left, double taskbarTop)
    {
        Left = left;
        Top = taskbarTop - Height;
    }

    private void PopulateDefaultContent()
    {
        var recents = _applicationManager.GetRecentApplications().ToList();
        RecentHeader.Visibility = recents.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        RecentListBox.Visibility = recents.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        RecentListBox.ItemsSource = recents;

        var rootCategories = _categoryManager.GetRootCategories().ToList();
        CategoriesHeader.Visibility = rootCategories.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        CategoriesPanel.Children.Clear();
        foreach (var category in rootCategories)
        {
            var button = new Button
            {
                Content = BuildCategoryButtonContent(category),
                Style = (Style)FindResource("TaskbarButtonStyle"),
                Margin = new Thickness(0, 0, 4, 4),
                Tag = category,
            };
            button.Click += (_, _) =>
            {
                var menu = _categoryMenuBuilder.Build(category);
                menu.PlacementTarget = button;
                menu.Placement = PlacementMode.Bottom;
                menu.IsOpen = true;
            };
            CategoriesPanel.Children.Add(button);
        }

        AllAppsListBox.ItemsSource = _applicationManager.Applications
            .OrderBy(a => a.Name, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
    }

    /// <summary>Icon + name for a category button — was plain "{glyph} {name}"
    /// text, which meant a custom image icon set via Manage Category never showed
    /// up here even though the taskbar's own category buttons already honored it
    /// (round feedback: "why is the category icon inside start not follow icon
    /// that i have change in categories?"). Mirrors
    /// TaskbarWindow.BuildCategoryIconContent's fallback: a custom image if one's
    /// set, otherwise the emoji glyph (or a generic folder icon if neither).</summary>
    private UIElement BuildCategoryButtonContent(ApplicationCategory category)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal };

        if (category.CustomIconPath is { } path && CategoryIconService.TryLoadIcon(path, out var bitmap))
        {
            content.Children.Add(new Image { Width = 16, Height = 16, Margin = new Thickness(0, 0, 4, 0), Source = bitmap });
        }
        else
        {
            content.Children.Add(new TextBlock
            {
                Text = string.IsNullOrEmpty(category.IconGlyph) ? "📁" : category.IconGlyph,
                Margin = new Thickness(0, 0, 4, 0),
            });
        }

        content.Children.Add(new TextBlock { Text = category.Name, VerticalAlignment = VerticalAlignment.Center });
        return content;
    }

    private void SearchBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        var query = SearchBox.Text;
        if (string.IsNullOrWhiteSpace(query))
        {
            ResultsPanel.Visibility = Visibility.Collapsed;
            DefaultPanel.Visibility = Visibility.Visible;
            return;
        }

        DefaultPanel.Visibility = Visibility.Collapsed;
        ResultsPanel.Visibility = Visibility.Visible;
        ResultsListBox.ItemsSource = ApplicationSearch.Search(query, _applicationManager.Applications, _categoryManager);
    }

    private void SearchBox_PreviewKeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Escape:
                Close();
                e.Handled = true;
                break;

            case Key.Down:
                var target = ResultsPanel.Visibility == Visibility.Visible ? ResultsListBox : AllAppsListBox;
                if (target.Items.Count > 0)
                {
                    target.SelectedIndex = 0;
                    target.Focus();
                }
                e.Handled = true;
                break;

            case Key.Enter:
                if (ResultsPanel.Visibility == Visibility.Visible && ResultsListBox.Items.Count > 0)
                {
                    LaunchAndClose((ApplicationEntry)ResultsListBox.Items[0]);
                    e.Handled = true;
                }
                break;
        }
    }

    private void AttachLaunchBehavior(ListBox listBox)
    {
        listBox.PreviewMouseLeftButtonUp += (_, e) =>
        {
            if (FindAncestorListBoxItem(e.OriginalSource as DependencyObject) is { DataContext: ApplicationEntry entry })
                LaunchAndClose(entry);
        };

        listBox.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter && listBox.SelectedItem is ApplicationEntry entry)
            {
                LaunchAndClose(entry);
                e.Handled = true;
            }
            else if (e.Key == Key.Escape)
            {
                Close();
                e.Handled = true;
            }
        };
    }

    private void LaunchAndClose(ApplicationEntry entry)
    {
        _applicationManager.Launch(entry);
        Close();
    }

    private static ListBoxItem? FindAncestorListBoxItem(DependencyObject? source)
    {
        while (source is not null && source is not ListBoxItem)
            source = VisualTreeHelper.GetParent(source);

        return source as ListBoxItem;
    }

    private void SettingsButton_Click(object sender, RoutedEventArgs e)
    {
        _openSettings();
        Close();
    }

    private void PowerButton_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        AddPowerItem(menu, "Lock", PowerActionService.Lock);
        AddPowerItem(menu, "Sign out", PowerActionService.SignOut);
        AddPowerItem(menu, "Restart", PowerActionService.Restart);
        AddPowerItem(menu, "Shut down", PowerActionService.Shutdown);

        menu.PlacementTarget = PowerButton;
        menu.Placement = PlacementMode.Top;
        menu.IsOpen = true;
    }

    private static void AddPowerItem(ContextMenu menu, string header, Action action)
    {
        var item = new MenuItem { Header = header };
        item.Click += (_, _) => action();
        menu.Items.Add(item);
    }
}
