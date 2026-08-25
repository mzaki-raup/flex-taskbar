using System.IO;
using System.Windows;
using System.Windows.Controls;
using FlexTaskbar.Applications;

namespace FlexTaskbar.Menus;

/// <summary>
/// Real (not stubbed) category management surface reachable from a category's flyout
/// menu (spec Section 10: Rename, New Subcategory, Delete, Change Icon).
/// </summary>
public partial class CategoryManagementWindow : Window
{
    // A curated set rather than arbitrary image-file picking (spec Section 49) —
    // real, working custom icons without the extra scope of a full file picker +
    // image-to-icon conversion pipeline.
    private static readonly string[] IconChoices =
    {
        "\U0001F4C1", "\U0001F4BB", "\U0001F916", "\U0001F3A8", "\U0001F310",
        "\U0001F4C4", "\U0001F3AE", "\U0001F3A5", "\U0001F527", "⚙️",
        "\U0001F4E6", "\U0001F4CA", "\U0001F4AC", "\U0001F4E7", "\U0001F511",
        "☁️", "\U0001F4F1", "\U0001F3E0", "❤️", "⭐",
    };

    private readonly ApplicationCategory _category;
    private readonly CategoryManager _categoryManager;
    private readonly ApplicationManager _applicationManager;

    public CategoryManagementWindow(ApplicationCategory category, CategoryManager categoryManager, ApplicationManager applicationManager)
    {
        InitializeComponent();
        _category = category;
        _categoryManager = categoryManager;
        _applicationManager = applicationManager;

        HeaderText.Text = _category.CustomIconPath is not null ? $"\U0001F5BC {_category.Name}" : $"{_category.IconGlyph} {_category.Name}";
        BuildIconPicker();
    }

    private void BuildIconPicker()
    {
        IconPickerPanel.Children.Clear();
        foreach (var glyph in IconChoices)
        {
            var isSelected = glyph == _category.IconGlyph;
            var button = new Button
            {
                Content = glyph,
                Width = 32,
                Height = 32,
                Margin = new Thickness(0, 0, 4, 4),
                FontSize = 14,
                Style = (Style)FindResource("TaskbarButtonStyle"),
                Background = isSelected ? (System.Windows.Media.Brush)FindResource("ItemPressedBrush") : System.Windows.Media.Brushes.Transparent,
            };
            button.Click += (_, _) =>
            {
                _categoryManager.SetIcon(_category.Id, glyph);
                HeaderText.Text = $"{glyph} {_category.Name}";
                BuildIconPicker(); // rebuild so the newly-selected glyph highlights
            };
            IconPickerPanel.Children.Add(button);
        }
    }

    private void ChooseIconFile_Click(object sender, RoutedEventArgs e)
    {
        // The whole method is wrapped now, not just the icon-processing part —
        // round-6 feedback ("still not working... nothing happens") persisted
        // after widening the catch around SetCustomIcon, which meant the real
        // failure was elsewhere. Root cause: dialog.ShowDialog(this) itself was
        // OUTSIDE any try/catch, so its exception had nowhere to go but the
        // app-wide DispatcherUnhandledException safety net — invisible to the
        // user, same symptom as the earlier bug, just one call earlier.
        //
        // Round 9 (live user report, with the exact error text this time):
        // Microsoft.Win32.OpenFileDialog — WPF's wrapper, which always uses the
        // modern COM-based picker (IFileOpenDialog) on Vista+ — failed with
        // "Retrieving the COM class factory for component with CLSID
        // {DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7} failed... 0x8007007E The
        // specified module could not be found." That HRESULT means the COM
        // registration itself exists but the DLL it points to can't load — a
        // broken/missing system shell component, not anything in FlexTaskbar's
        // own manifest or code. Confirmed system-wide on the reporting user's
        // machine (their OS's own screenshot-save dialog, a completely
        // unrelated app, failed the same way) — not fixable from inside this
        // app. Switched to System.Windows.Forms.OpenFileDialog with
        // AutoUpgradeEnabled = false, which forces the legacy, non-COM
        // GetOpenFileName-based picker instead — it doesn't touch
        // IFileOpenDialog at all, so it sidesteps whatever's broken with that
        // component entirely. (System.Windows.Forms is already a project
        // dependency — see Tray/TrayIconService.cs — referenced here fully
        // qualified rather than via a blanket `using`, since this file's own
        // Button/etc. usages are the WPF ones and a blanket using would collide.)
        try
        {
            using var dialog = new System.Windows.Forms.OpenFileDialog
            {
                Title = "Choose a category icon",
                Filter = "Image files (*.png;*.ico;*.svg)|*.png;*.ico;*.svg",
                AutoUpgradeEnabled = false,
            };

            var ownerHandle = new System.Windows.Interop.WindowInteropHelper(this).Handle;
            if (dialog.ShowDialog(new FlexTaskbar.Utilities.Win32WindowHandle(ownerHandle)) != System.Windows.Forms.DialogResult.OK)
                return;

            _categoryManager.SetCustomIcon(_category.Id, dialog.FileName);
            HeaderText.Text = $"\U0001F5BC {_category.Name}"; // 🖼 stands in for the now-custom icon in this text-only header
            BuildIconPicker(); // rebuild so no emoji glyph shows as "selected" anymore
        }
        catch (Exception ex)
        {
            // Deliberately broad — see the method's opening comment on why a
            // narrower catch here already proved insufficient once.
            MessageBox.Show(this, $"Couldn't use that file as an icon: {ex.Message}", "FlexTaskbar", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void Rename_Click(object sender, RoutedEventArgs e)
    {
        var newName = TextInputWindow.Prompt(this, "New name:", _category.Name);
        if (newName is null)
            return;

        _categoryManager.RenameCategory(_category.Id, newName);
        HeaderText.Text = _category.CustomIconPath is not null ? $"\U0001F5BC {newName}" : $"{_category.IconGlyph} {newName}";
    }

    private void AddSubcategory_Click(object sender, RoutedEventArgs e)
    {
        var name = TextInputWindow.Prompt(this, "New subcategory name:");
        if (name is null)
            return;

        _categoryManager.AddCategory(name, _category.Id);
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        var result = MessageBox.Show(
            this,
            $"Delete \"{_category.Name}\"? Subcategories move up a level and its apps become uncategorized — nothing is uninstalled or lost.",
            "Delete Category",
            MessageBoxButton.YesNo,
            MessageBoxImage.Warning);

        if (result != MessageBoxResult.Yes)
            return;

        _categoryManager.DeleteCategory(_category.Id, _applicationManager);
        Close();
    }

    private void Close_Click(object sender, RoutedEventArgs e)
    {
        Close();
    }
}
