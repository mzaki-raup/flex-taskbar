using System.Runtime.InteropServices;

namespace FlexTaskbar.Native;

/// <summary>Shared structs used across multiple Native/*.cs files (spec Section 63 — centralized P/Invoke).</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct RECT
{
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
}
