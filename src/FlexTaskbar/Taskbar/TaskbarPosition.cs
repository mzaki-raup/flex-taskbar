namespace FlexTaskbar.Taskbar;

/// <summary>
/// Where the taskbar docks on screen. Only <see cref="Bottom"/> and
/// <see cref="Top"/> are supported for now (see spec Section 20); the
/// enum includes Left/Right so the layout code has somewhere to grow
/// into without a breaking change later.
/// </summary>
public enum TaskbarPosition
{
    Bottom,
    Top,
    Left,
    Right,
}
