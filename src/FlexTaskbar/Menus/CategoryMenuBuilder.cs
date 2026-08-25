using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using FlexTaskbar.Applications;
using FlexTaskbar.Utilities;

namespace FlexTaskbar.Menus;

/// <summary>
/// Builds a category's flyout as a real, native <see cref="ContextMenu"/>/<see cref="MenuItem"/>
/// tree (spec Section 6) rather than hand-rolled Popups. This gets submenu-on-hover,
/// arrow-key navigation, Escape-to-close, and click-outside-to-dismiss "for free" from
/// WPF's built-in Menu behavior (Section 18 keyboard navigation), which also happens
/// to be the more native-feeling choice (spec Section 48: "should feel like a native
/// Windows component").
/// </summary>
public sealed class CategoryMenuBuilder
{
    // "Slightly bigger" than the 16x16 default MenuItem icon column, and rendered
    // as a horizontal icon+name tile (matching the icon-first visual language the
    // taskbar's own running/pinned app buttons use) rather than a plain text row.
    // Was 24 — bumped to match the taskbar's own icon-size fix (round feedback:
    // "apply this also inside categories list of apps") so this Start-window
    // category flyout reads at the same visual weight as the taskbar's.
    private const double AppTileIconSize = 32;

    private readonly ApplicationManager _applicationManager;
    private readonly CategoryManager _categoryManager;
    private readonly Window _owner;

    public CategoryMenuBuilder(ApplicationManager applicationManager, CategoryManager categoryManager, Window owner)
    {
        _applicationManager = applicationManager;
        _categoryManager = categoryManager;
        _owner = owner;
    }

    public ContextMenu Build(ApplicationCategory category)
    {
        // A ContextMenu is its own separate visual-tree root (like a Popup) —
        // without these, the app-tile icons here can land on sub-pixel
        // boundaries and blur regardless of source resolution or
        // BitmapScalingMode (round feedback: "still not sharp" after both of
        // those were already fixed — this dialog just wasn't covered yet).
        var menu = new ContextMenu { UseLayoutRounding = true, SnapsToDevicePixels = true };
        Populate(menu.Items, category);
        return menu;
    }

    private void Populate(ItemCollection items, ApplicationCategory category)
    {
        var subcategories = _categoryManager.GetChildren(category.Id).ToList();
        foreach (var sub in subcategories)
        {
            var subItem = new MenuItem { Header = $"{sub.IconGlyph} {sub.Name}" };
            Populate(subItem.Items, sub);
            items.Add(subItem);
        }

        // Same ordering the taskbar's own category flyout uses (see
        // ApplicationManager.GetApplicationsInCategory) so a drag-reordered app
        // list looks consistent between the two surfaces.
        var apps = _applicationManager.GetApplicationsInCategory(category.Id).ToList();

        if (subcategories.Count > 0 && apps.Count > 0)
            items.Add(new Separator());

        if (apps.Count > 0)
            items.Add(BuildAppsRow(apps));

        if (subcategories.Count > 0 || apps.Count > 0)
            items.Add(new Separator());

        var manageItem = new MenuItem { Header = "⚙ Manage Category" };
        manageItem.Click += (_, _) =>
        {
            var window = new CategoryManagementWindow(category, _categoryManager, _applicationManager) { Owner = _owner };
            window.ShowDialog();
        };
        items.Add(manageItem);
    }

    /// <summary>Renders every app in this category as one horizontal row of
    /// tiles (round feedback: "apps icon inside category show as horizontal, not
    /// vertical" — previously each app was its own MenuItem, which stacked one per
    /// line no matter how the tile itself was styled) instead of a vertical list.
    /// Wrapping is handled by the WrapPanel's own MaxWidth once a category has
    /// more apps than fit on one line. A single MenuItem hosts the whole row — WPF
    /// auto-wraps any non-MenuItem object added to a menu's Items in a MenuItem
    /// whose Header is that object, so this is the same mechanism the per-app
    /// MenuItems used, just with one shared container instead of many. Each
    /// tile's own Button click handling (Button marks the event Handled) launches
    /// the right app instead of the row's own MenuItem intercepting every click
    /// identically.</summary>
    private MenuItem BuildAppsRow(List<ApplicationEntry> apps)
    {
        var row = new WrapPanel { Orientation = Orientation.Horizontal, MaxWidth = 220 };
        foreach (var app in apps)
            row.Children.Add(BuildAppTileButton(app));

        return new MenuItem { Header = row };
    }

    /// <summary>Icon above, name below — reserves two lines for the name (round
    /// feedback: "apps inside category in Start have 2 row of apps text name")
    /// so a short one-line name doesn't sit higher than a wrapped two-line one and
    /// tiles in the same row look misaligned, same fix already applied to the
    /// taskbar's own category flyout tiles. Horizontal margin between tiles (round
    /// feedback: "some margin left and right").</summary>
    private Button BuildAppTileButton(ApplicationEntry app)
    {
        var tileContent = new StackPanel { Orientation = Orientation.Vertical, HorizontalAlignment = HorizontalAlignment.Center };
        if (ApplicationIconConverter.IconService.GetIcon(app) is { } icon)
        {
            tileContent.Children.Add(new Image { Width = AppTileIconSize, Height = AppTileIconSize, Margin = new Thickness(0, 0, 0, 4), Source = icon });
        }
        tileContent.Children.Add(new TextBlock
        {
            Text = app.Name,
            FontSize = 11,
            TextAlignment = TextAlignment.Center,
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 76,
            MinHeight = 28, // two lines' worth, even for a name that only needs one
            // TaskbarButtonStyle's Foreground (white, for the dark taskbar
            // background) would otherwise be inherited here — but this tile sits
            // inside a *native* ContextMenu (the Start window's category flyout),
            // which has no custom dark styling and renders with the system's
            // default light menu chrome. White-on-light made the name invisible
            // (round feedback: "i cannot see the text inside apps in categories
            // in Start"). SystemColors.MenuTextBrush matches whatever the actual
            // native menu background is, light or dark/high-contrast.
            Foreground = SystemColors.MenuTextBrush,
        });

        var button = new Button
        {
            Style = (Style)Application.Current.FindResource("TaskbarButtonStyle"),
            Padding = new Thickness(4),
            Margin = new Thickness(6, 2, 6, 2),
            ToolTip = app.Name,
            Content = tileContent,
        };
        AutomationProperties.SetName(button, app.Name);
        button.Click += (_, _) => _applicationManager.Launch(app);
        return button;
    }
}
