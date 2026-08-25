namespace FlexTaskbar.Utilities;

/// <summary>Lets a WPF window act as the owner of a WinForms dialog — WPF's Window
/// doesn't implement IWin32Window itself, but wrapping its real HWND (available once
/// the window has loaded) is all IWin32Window actually needs. Shared by every place
/// that opens a WinForms OpenFileDialog (see CategoryManagementWindow.xaml.cs for why
/// that dialog, not WPF's own, is used) so an icon file picker only has to be wired
/// up once per call site.</summary>
public sealed class Win32WindowHandle : System.Windows.Forms.IWin32Window
{
    public Win32WindowHandle(IntPtr handle) => Handle = handle;
    public IntPtr Handle { get; }
}
