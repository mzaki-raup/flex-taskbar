using System.Globalization;
using System.Windows.Data;
using FlexTaskbar.Applications;

namespace FlexTaskbar.Utilities;

/// <summary>
/// Binds an <see cref="ApplicationEntry"/> directly to an icon <c>Image.Source</c> in
/// XAML. Backed by a single shared <see cref="ApplicationIconService"/> instance so
/// its in-memory/disk icon cache (Section 27) is actually shared across every list
/// that renders application icons, instead of each list re-extracting independently.
/// </summary>
public sealed class ApplicationIconConverter : IValueConverter
{
    public static readonly ApplicationIconService IconService = new();

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        return value is ApplicationEntry entry ? IconService.GetIcon(entry) : null;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotSupportedException();
    }
}
