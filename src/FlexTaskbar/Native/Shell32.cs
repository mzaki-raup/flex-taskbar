using System.Runtime.InteropServices;

namespace FlexTaskbar.Native;

/// <summary>
/// Centralized P/Invoke + COM-interop surface for shell32.dll functionality used by
/// application discovery and icon extraction (spec Section 63/64).
///
/// Two distinct native features live here:
///   1. SHGetFileInfo — resolves an executable's shell icon without loading the file
///      as a managed image (used by ApplicationIconService).
///   2. IShellLinkW / IPersistFile — reads the target path, arguments, working
///      directory, and icon location out of a .lnk shortcut (used by ShellIntegration
///      to resolve Start Menu / Desktop shortcuts during scanning).
/// </summary>
internal static class Shell32
{
    public const uint SHGFI_ICON = 0x100;
    public const uint SHGFI_LARGEICON = 0x0;
    public const uint SHGFI_SMALLICON = 0x1;

    /// <summary>Returns the system image list index in SHFILEINFO.iIcon instead
    /// of a ready-made HICON — the first half of the SHGetImageList-based
    /// extraction path (see ApplicationIconService), which is what's actually
    /// needed for a sharp icon: SHGFI_LARGEICON alone caps out at the classic
    /// "large" system icon size (historically 32x32, DPI-unaware), which round
    /// feedback ("icon category and apps shortcut still not sharp") traced back
    /// to — every *custom* icon was already fixed (higher-res rasterization +
    /// high-quality downscaling), but apps without one still fell back to this
    /// low-res native extraction, which those two fixes never touched.</summary>
    public const uint SHGFI_SYSICONINDEX = 0x4000;

    /// <summary>SHIL_JUMBO — the 256x256 system image list, populated on-demand
    /// by the shell (Explorer's own "Extra Large icons" view uses the same one).
    /// Passed to SHGetImageList. NOT used by default (see SHIL_EXTRALARGE below)
    /// — kept for reference/future use.</summary>
    public const int SHIL_JUMBO = 0x4;

    /// <summary>SHIL_EXTRALARGE — the 48x48 system image list. Used instead of
    /// SHIL_JUMBO: when an app's icon resource doesn't actually contain a true
    /// high-resolution image, the shell centers the smaller source within the
    /// Jumbo list's 256x256 canvas *with transparent padding* rather than
    /// upscaling it to fill the space (this is deliberate, documented Explorer
    /// "Extra large icons" behavior, not a bug) — so once that gets scaled back
    /// down for a small list icon, apps with a true 256x256 resource look normal
    /// while everything else looks tiny, inconsistently, entry to entry (round
    /// feedback: "some of the icon showing smallest icon, some showing normal").
    /// Far more apps have a genuine 48x48 resource than a genuine 256x256 one, so
    /// this list is populated with properly-scaled (not padded) icons much more
    /// consistently, while still comfortably exceeding this app's largest actual
    /// render size (32-36px tiles).</summary>
    public const int SHIL_EXTRALARGE = 0x2;

    public const int ILD_TRANSPARENT = 0x1;

    public static readonly Guid IID_IImageList = new("46EB5926-582E-4017-9FDF-E8998DAA0950");

    [DllImport("shell32.dll")]
    public static extern int SHGetImageList(int iImageList, ref Guid riid, out IImageList ppv);

    /// <summary>
    /// Full method list, in COM vtable order — .NET COM interop dispatches by
    /// declared position, not by name, so every member ahead of
    /// <see cref="GetIcon"/> in the real IImageList vtable has to be declared
    /// here too even though nothing else in this app calls them, or GetIcon would
    /// silently invoke the wrong slot.
    /// </summary>
    [ComImport]
    [Guid("46EB5926-582E-4017-9FDF-E8998DAA0950")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IImageList
    {
        [PreserveSig] int Add(IntPtr hbmImage, IntPtr hbmMask, ref int pi);
        [PreserveSig] int ReplaceIcon(int i, IntPtr hicon, ref int pi);
        [PreserveSig] int SetOverlayImage(int iImage, int iOverlay);
        [PreserveSig] int Replace(int i, IntPtr hbmImage, IntPtr hbmMask);
        [PreserveSig] int AddMasked(IntPtr hbmImage, int crMask, ref int pi);
        [PreserveSig] int Draw(IntPtr pimldp);
        [PreserveSig] int Remove(int i);
        [PreserveSig] int GetIcon(int i, int flags, out IntPtr picon);
        [PreserveSig] int GetImageInfo(int i, IntPtr pImageInfo);
        [PreserveSig] int Copy(int iDst, IntPtr punkSrc, int iSrc, int uFlags);
        [PreserveSig] int Merge(int i1, IntPtr punk2, int i2, int dx, int dy, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int Clone(ref Guid riid, out IntPtr ppv);
        [PreserveSig] int GetImageRect(int i, IntPtr prc);
        [PreserveSig] int GetIconSize(out int cx, out int cy);
        [PreserveSig] int SetIconSize(int cx, int cy);
        [PreserveSig] int GetImageCount(out int pi);
        [PreserveSig] int SetImageCount(int uNewCount);
        [PreserveSig] int SetBkColor(int clrBk, out int pclr);
        [PreserveSig] int GetBkColor(out int pclr);
        [PreserveSig] int BeginDrag(int iTrack, int dxHotspot, int dyHotspot);
        [PreserveSig] int EndDrag();
        [PreserveSig] int DragEnter(IntPtr hwndLock, int x, int y);
        [PreserveSig] int DragLeave(IntPtr hwndLock);
        [PreserveSig] int DragMove(int x, int y);
        [PreserveSig] int SetDragCursorImage(IntPtr punk, int iDrag, int dxHotspot, int dyHotspot);
        [PreserveSig] int DragShowNolock(bool fShow);
        [PreserveSig] int GetDragImage(IntPtr ppt, IntPtr pptHotspot, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int GetItemFlags(int i, out int dwFlags);
        [PreserveSig] int GetOverlayImage(int iOverlay, out int piIndex);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct WIN32_FIND_DATAW
    {
        public uint dwFileAttributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME ftCreationTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME ftLastAccessTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME ftLastWriteTime;
        public uint nFileSizeHigh;
        public uint nFileSizeLow;
        public uint dwReserved0;
        public uint dwReserved1;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string cFileName;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 14)]
        public string cAlternateFileName;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct SHFILEINFO
    {
        public IntPtr hIcon;
        public int iIcon;
        public uint dwAttributes;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szDisplayName;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)]
        public string szTypeName;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, ExactSpelling = false)]
    public static extern IntPtr SHGetFileInfo(
        string pszPath,
        uint dwFileAttributes,
        ref SHFILEINFO psfi,
        uint cbFileInfo,
        uint uFlags);

    /// <summary>
    /// COM class ID for the Shell Link object (CLSID_ShellLink). Instantiating this
    /// and casting to <see cref="IShellLinkW"/> / <see cref="System.Runtime.InteropServices.ComTypes.IPersistFile"/>
    /// is the standard documented way to read .lnk files without shelling out to
    /// WScript.Shell.
    /// </summary>
    [ComImport]
    [Guid("00021401-0000-0000-C000-000000000046")]
    public class ShellLinkCoClass
    {
    }

    [ComImport]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    [Guid("000214F9-0000-0000-C000-000000000046")]
    public interface IShellLinkW
    {
        void GetPath(
            [Out] [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszFile,
            int cchMaxPath,
            out WIN32_FIND_DATAW pfd,
            uint fFlags);

        void GetIDList(out IntPtr ppidl);
        void SetIDList(IntPtr pidl);

        void GetDescription(
            [Out] [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszName,
            int cchMaxName);

        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string pszName);

        void GetWorkingDirectory(
            [Out] [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszDir,
            int cchMaxPath);

        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string pszDir);

        void GetArguments(
            [Out] [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszArgs,
            int cchMaxPath);

        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string pszArgs);

        void GetHotkey(out short pwHotkey);
        void SetHotkey(short wHotkey);
        void GetShowCmd(out int piShowCmd);
        void SetShowCmd(int iShowCmd);

        void GetIconLocation(
            [Out] [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszIconPath,
            int cchIconPath,
            out int piIcon);

        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string pszIconPath, int iIcon);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pszPathRel, uint dwReserved);
        void Resolve(IntPtr hwnd, uint fFlags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string pszFile);
    }
}
