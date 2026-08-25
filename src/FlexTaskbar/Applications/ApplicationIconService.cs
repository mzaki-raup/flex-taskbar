using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media.Imaging;
using FlexTaskbar.Native;
using FlexTaskbar.Utilities;

namespace FlexTaskbar.Applications;

/// <summary>
/// Extracts native shell icons for application entries (spec Section 35) via
/// SHGetFileInfo, and disk-caches the result as PNG under
/// %LOCALAPPDATA%\FlexTaskbar\IconCache so icons are never re-extracted on every run
/// (Section 27 performance budget). An in-memory cache avoids re-decoding the PNG
/// for entries requested more than once in the same session.
///
/// Icon extraction always targets the *resolved* executable path, never the .lnk
/// itself — SHGetFileInfo on a .lnk applies the shell's link-arrow overlay, which
/// isn't wanted here since the taskbar UI already communicates "this is a shortcut"
/// contextually.
/// </summary>
public sealed class ApplicationIconService
{
    private readonly Dictionary<string, BitmapSource?> _memoryCache = new(StringComparer.OrdinalIgnoreCase);

    public BitmapSource? GetIcon(ApplicationEntry entry)
    {
        // A user-picked custom icon always wins, regardless of LaunchKind — this is
        // what lets web apps (which have no exe to extract an icon from at all) get
        // an icon too, not just normal executables.
        if (entry.CustomIconPath is { } customPath && CategoryIconService.TryLoadIcon(customPath, out var custom))
            return custom;

        if (entry.LaunchKind != ApplicationLaunchKind.Executable)
            return null;

        return GetIconForExecutable(entry.ExecutablePath);
    }

    /// <summary>
    /// Same extraction/caching path as <see cref="GetIcon"/>, keyed directly on an
    /// executable path — used by running-window buttons (Phase 4), which only know a
    /// process's exe path, not an <see cref="ApplicationEntry"/>.
    /// </summary>
    public BitmapSource? GetIconForExecutable(string? sourcePath)
    {
        if (string.IsNullOrEmpty(sourcePath) || !File.Exists(sourcePath))
            return null;

        if (_memoryCache.TryGetValue(sourcePath, out var cached))
            return cached;

        var icon = LoadFromDiskCache(sourcePath) ?? ExtractAndCache(sourcePath);
        _memoryCache[sourcePath] = icon;
        return icon;
    }

    private static string GetCacheFilePath(string sourcePath)
    {
        // Keyed on path + last-write time so a replaced/updated executable gets a
        // fresh icon instead of serving a stale cached one indefinitely.
        var lastWriteTicks = File.GetLastWriteTimeUtc(sourcePath).Ticks;
        var key = $"{sourcePath.ToLowerInvariant()}|{lastWriteTicks}";
        var hash = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(key)));
        return Path.Combine(AppPaths.IconCacheDirectory, hash + ".png");
    }

    private static BitmapSource? LoadFromDiskCache(string sourcePath)
    {
        var cachePath = GetCacheFilePath(sourcePath);
        if (!File.Exists(cachePath))
            return null;

        try
        {
            var bitmap = new BitmapImage();
            bitmap.BeginInit();
            bitmap.CacheOption = BitmapCacheOption.OnLoad;
            bitmap.UriSource = new Uri(cachePath, UriKind.Absolute);
            bitmap.EndInit();
            bitmap.Freeze();
            return bitmap;
        }
        catch (Exception ex) when (ex is IOException or NotSupportedException or FileFormatException)
        {
            return null;
        }
    }

    private static BitmapSource? ExtractAndCache(string sourcePath)
    {
        if ((ExtractHighResIcon(sourcePath) ?? ExtractLargeIcon(sourcePath)) is not { } hIcon)
            return null;

        try
        {
            var bitmapSource = Imaging.CreateBitmapSourceFromHIcon(
                hIcon, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            bitmapSource.Freeze();

            SaveToDiskCache(sourcePath, bitmapSource);
            return bitmapSource;
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException)
        {
            return null;
        }
        finally
        {
            User32.DestroyIcon(hIcon);
        }
    }

    /// <summary>
    /// 48x48 "Extra large" icon via SHGetImageList — the same mechanism
    /// Explorer's own "Large icons" view uses. Plain SHGFI_LARGEICON (the
    /// fallback, <see cref="ExtractLargeIcon"/>) caps out at the classic,
    /// DPI-unaware ~32x32 system icon size, which round feedback ("icon category
    /// and apps shortcut still not sharp") traced back to: custom-picked icons
    /// were already fixed (higher rasterization resolution + high-quality
    /// downscaling), but apps without a custom icon still fell back to this
    /// low-res native extraction, which those fixes never touched.
    ///
    /// Deliberately SHIL_EXTRALARGE, not SHIL_JUMBO (256x256) — an app whose icon
    /// resource doesn't actually contain a true high-res image gets its smaller
    /// source centered in the Jumbo list's canvas *with transparent padding*
    /// rather than upscaled to fill it (deliberate, documented Explorer behavior,
    /// not a bug), so once scaled back down for a small list icon, apps with a
    /// genuine 256x256 resource looked normal while everything else looked tiny —
    /// inconsistently, entry to entry (round feedback: "some of the icon showing
    /// smallest icon, some showing normal"). Far more apps have a true 48x48
    /// resource than a true 256x256 one, so this list is populated with
    /// properly-scaled icons much more consistently, while still comfortably
    /// exceeding this app's largest actual render size (32-36px tiles).
    ///
    /// Returns null (falling back to <see cref="ExtractLargeIcon"/>) for anything
    /// this doesn't cleanly support — deliberately broad catch, matching this
    /// file's existing "never let icon extraction take the taskbar down"
    /// contract.
    /// </summary>
    private static IntPtr? ExtractHighResIcon(string sourcePath)
    {
        try
        {
            var info = new Shell32.SHFILEINFO();
            var handle = Shell32.SHGetFileInfo(
                sourcePath, 0, ref info, (uint)Marshal.SizeOf<Shell32.SHFILEINFO>(),
                Shell32.SHGFI_SYSICONINDEX);

            if (handle == IntPtr.Zero)
                return null;

            var iid = Shell32.IID_IImageList;
            if (Shell32.SHGetImageList(Shell32.SHIL_EXTRALARGE, ref iid, out var imageList) != 0 || imageList is null)
                return null;

            return imageList.GetIcon(info.iIcon, Shell32.ILD_TRANSPARENT, out var hIcon) == 0 && hIcon != IntPtr.Zero
                ? hIcon
                : null;
        }
        catch (Exception ex) when (ex is COMException or InvalidCastException or DllNotFoundException or EntryPointNotFoundException)
        {
            return null;
        }
    }

    private static IntPtr? ExtractLargeIcon(string sourcePath)
    {
        var info = new Shell32.SHFILEINFO();
        var handle = Shell32.SHGetFileInfo(
            sourcePath, 0, ref info, (uint)Marshal.SizeOf<Shell32.SHFILEINFO>(),
            Shell32.SHGFI_ICON | Shell32.SHGFI_LARGEICON);

        return handle != IntPtr.Zero && info.hIcon != IntPtr.Zero ? info.hIcon : null;
    }

    private static void SaveToDiskCache(string sourcePath, BitmapSource bitmapSource)
    {
        try
        {
            var cachePath = GetCacheFilePath(sourcePath);
            var encoder = new PngBitmapEncoder();
            encoder.Frames.Add(BitmapFrame.Create(bitmapSource));
            using var stream = File.Create(cachePath);
            encoder.Save(stream);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Non-fatal: the icon just won't be cached this run and will be
            // re-extracted next time (Section 34 — never crash over a caching failure).
        }
    }
}
