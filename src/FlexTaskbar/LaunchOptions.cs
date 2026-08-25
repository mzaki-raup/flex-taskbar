namespace FlexTaskbar;

/// <summary>
/// Parsed command-line switches. See Section 54 of the spec for the full
/// planned surface; only --safe-mode and --disable are wired up in Phase 1,
/// the rest are recognized but not yet implemented (TODO, later phases).
/// </summary>
public sealed class LaunchOptions
{
    public bool SafeMode { get; private init; }
    public bool Disable { get; private init; }
    public bool OpenSettings { get; private init; }
    public bool Restart { get; private init; }
    public bool Reset { get; private init; }

    public static LaunchOptions Parse(string[] args)
    {
        return new LaunchOptions
        {
            SafeMode = Has(args, "--safe-mode"),
            Disable = Has(args, "--disable"),
            OpenSettings = Has(args, "--settings"),
            Restart = Has(args, "--restart"),
            Reset = Has(args, "--reset"),
        };
    }

    private static bool Has(string[] args, string flag)
    {
        foreach (var arg in args)
        {
            if (string.Equals(arg, flag, System.StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }
}
