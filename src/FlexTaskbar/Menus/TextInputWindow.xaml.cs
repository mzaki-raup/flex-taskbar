using System.Windows;

namespace FlexTaskbar.Menus;

/// <summary>
/// Minimal reusable prompt (single text field + OK/Cancel) used for category
/// rename/create. Deliberately plain — the polished Settings/category management UI
/// is a later phase; this exists so "New Subcategory" etc. are real, working
/// features now rather than deferred stubs (spec's "No Fake Features" rule).
/// </summary>
public partial class TextInputWindow : Window
{
    public string InputText => InputBox.Text.Trim();

    public TextInputWindow(string prompt, string initialValue = "")
    {
        InitializeComponent();
        PromptText.Text = prompt;
        InputBox.Text = initialValue;
        Loaded += (_, _) =>
        {
            InputBox.Focus();
            InputBox.SelectAll();
        };
    }

    /// <summary>Shows the dialog and returns the trimmed input, or null if cancelled/blank.</summary>
    public static string? Prompt(Window owner, string title, string initialValue = "")
    {
        var window = new TextInputWindow(title, initialValue) { Owner = owner };
        var result = window.ShowDialog();
        return result == true && !string.IsNullOrWhiteSpace(window.InputText) ? window.InputText : null;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = true;
    }

    private void Cancel_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = false;
    }
}
