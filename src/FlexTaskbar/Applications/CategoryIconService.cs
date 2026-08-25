using System.IO;
using System.Windows.Media.Imaging;
using FlexTaskbar.Utilities;
using Svg;
using SDBitmap = System.Drawing.Bitmap;
using SDGraphics = System.Drawing.Graphics;
using SDImageFormat = System.Drawing.Imaging.ImageFormat;
using SDInterpolationMode = System.Drawing.Drawing2D.InterpolationMode;

namespace FlexTaskbar.Applications;

/// <summary>
/// Handles user-selected custom icons for both categories and individual apps
/// (round-2 feedback for categories: "user can select icon for parent category from
/// disk (png, ico, svg). The icon selected will be optimized and saved by the app
/// independently"; later extended to per-app icons so any app added from All Apps —
/// exe or web app alike — can have its icon changed the same way). PNG/ICO/SVG are
/// all rasterized down to a fixed square size and written out as the app's own PNG
/// copy under <see cref="AppPaths.CategoryIconsDirectory"/> or
/// <see cref="AppPaths.ApplicationIconsDirectory"/> — never a reference back to the
/// original file, so a later move/rename/delete of whatever the user picked can't
/// break the icon.
/// </summary>
public static class CategoryIconService
{
    // Was 64 — every custom icon got downsampled to 64x64 on save regardless of
    // the source file's own resolution, so a 512x512 PNG pick still ended up soft
    // once WPF upscaled that fixed-size cache back out to fit HiDPI displays
    // (round feedback: "why is the icon for category and apps not sharp, even i
    // use 512x512 png icon?" — the bottleneck was this constant, not anything
    // about the source file). 256 comfortably covers every render size this app
    // actually uses (the largest is the ~32px icon picker button) even at 300%
    // Windows display scaling, while still keeping the saved PNG small.
    private const int IconSize = 256;

    /// <summary>Loads a previously-saved optimized icon for display. Returns false
    /// (with a null bitmap) if the file is missing or unreadable, so callers can
    /// fall back to the category's emoji glyph instead.</summary>
    public static bool TryLoadIcon(string path, out BitmapImage? bitmap)
    {
        bitmap = null;
        if (string.IsNullOrEmpty(path) || !File.Exists(path))
            return false;

        try
        {
            var image = new BitmapImage();
            image.BeginInit();
            image.CacheOption = BitmapCacheOption.OnLoad; // load fully now so the file isn't held open
            image.UriSource = new Uri(path, UriKind.Absolute);
            image.EndInit();
            image.Freeze();
            bitmap = image;
            return true;
        }
        catch (Exception ex) when (ex is IOException or NotSupportedException or FileFormatException)
        {
            return false;
        }
    }

    /// <summary>Rasterizes a user-selected PNG/ICO/SVG file to a fixed square size
    /// and saves it as this category's own PNG copy, returning the saved path.</summary>
    public static string SaveOptimizedIcon(string sourceFilePath, string categoryId) =>
        SaveOptimizedIcon(sourceFilePath, AppPaths.CategoryIconsDirectory, categoryId);

    /// <summary>Same rasterize-and-save behavior as the category overload, but for
    /// an arbitrary destination directory/id — used for per-app custom icons
    /// (<see cref="AppPaths.ApplicationIconsDirectory"/>, keyed by
    /// <see cref="ApplicationEntry.Id"/>).</summary>
    public static string SaveOptimizedIcon(string sourceFilePath, string destinationDirectory, string id)
    {
        var extension = Path.GetExtension(sourceFilePath).ToLowerInvariant();
        using var rendered = extension switch
        {
            ".svg" => RasterizeSvg(sourceFilePath),
            ".ico" => RasterizeIco(sourceFilePath),
            _ => RasterizeRaster(sourceFilePath),
        };

        var destPath = Path.Combine(destinationDirectory, id + ".png");
        rendered.Save(destPath, SDImageFormat.Png);
        return destPath;
    }

    private static SDBitmap RasterizeSvg(string path)
    {
        var document = SvgDocument.Open(path);
        document.Width = IconSize;
        document.Height = IconSize;
        return document.Draw(IconSize, IconSize);
    }

    /// <summary>.ico files need System.Drawing.Icon, not Bitmap's own constructor
    /// — Bitmap(string) doesn't reliably load the multi-resolution .ico format
    /// (this was silently failing for every .ico pick before — round-6
    /// feedback — since the exception it threw wasn't one of the types the
    /// caller's catch clause was watching for).</summary>
    private static SDBitmap RasterizeIco(string path)
    {
        using var icon = new System.Drawing.Icon(path, IconSize, IconSize);
        return Resize(icon.ToBitmap());
    }

    private static SDBitmap RasterizeRaster(string path)
    {
        using var source = new SDBitmap(path);
        return Resize(source);
    }

    private static SDBitmap Resize(SDBitmap source)
    {
        using (source)
        {
            var resized = new SDBitmap(IconSize, IconSize);
            using var g = SDGraphics.FromImage(resized);
            g.InterpolationMode = SDInterpolationMode.HighQualityBicubic;
            g.DrawImage(source, 0, 0, IconSize, IconSize);
            return resized;
        }
    }
}
