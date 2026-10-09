# Starts a built FlexTaskbar.exe and checks it through Windows itself: the
# bar's window, the screen space it reserves, and what it tells a screen
# reader. Everything here is what only a real Windows desktop can answer, so
# it covers the ground `cargo test` cannot (see the README's *What has been
# tested*).
#
#     powershell -ExecutionPolicy Bypass -File tools\smoke-test.ps1
#     powershell -ExecutionPolicy Bypass -File tools\smoke-test.ps1 -Exe path\to\FlexTaskbar.exe
#
# The app runs from a copy in a temporary folder with its own `FlexTaskbarData\`, so a
# FlexTaskbar already installed is left alone — and it is stopped again at the
# end whatever happens.
#
# Exits 0 if every check passed, 1 if any failed, and 2 if it could not run
# (no interactive desktop, or an instance already running). CI treats 2 as a
# skip: a runner started as a service has no desktop to draw on.
#
# The bar answers MSAA (`IAccessible`), which is what Narrator and NVDA read.
# It is not a native UI Automation provider, so this asks the way a screen
# reader does.

param(
    [string]$Exe = "$PSScriptRoot\..\target\release\FlexTaskbar.exe",
    [int]$StartSeconds = 15
)

$ErrorActionPreference = 'Stop'
$script:failures = @()
$script:checks = 0

function Check([string]$what, [bool]$ok, [string]$detail = '') {
    $script:checks++
    if ($ok) {
        Write-Host "  ok    $what"
    } else {
        Write-Host "  FAIL  $what$(if ($detail) { " - $detail" })"
        $script:failures += $what
    }
}

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Accessibility;

public class Smoke {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, ref RECT r, uint c);
  [DllImport("oleacc.dll", PreserveSig=false)]
  public static extern void AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid iid,
    [MarshalAs(UnmanagedType.Interface)] out object acc);

  public struct RECT { public int Left, Top, Right, Bottom; }

  public static List<IntPtr> Windows(string cls) {
    var found = new List<IntPtr>();
    EnumWindows((h, p) => {
      var sb = new StringBuilder(256); GetClassName(h, sb, 256);
      if (sb.ToString() == cls && IsWindowVisible(h)) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }

  public static RECT Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return r; }

  public static RECT WorkArea() {
    RECT r = new RECT();
    SystemParametersInfo(0x0030 /*SPI_GETWORKAREA*/, 0, ref r, 0);
    return r;
  }

  /// The window as a screen reader sees it: the root's name and role, then
  /// one line per child.
  public static List<string> Accessible(IntPtr h) {
    var o = new List<string>();
    Guid iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
    object raw;
    AccessibleObjectFromWindow(h, 0xFFFFFFFC /*OBJID_CLIENT*/, ref iid, out raw);
    var acc = (IAccessible)raw;
    o.Add(string.Format("ROOT\t{0}\t{1}\t{2}", acc.get_accName(0), acc.get_accRole(0), acc.accChildCount));
    for (int i = 1; i <= acc.accChildCount; i++) {
      string name = "", action = "";
      object role = null;
      int l = 0, t = 0, w = 0, ht = 0;
      try { name = acc.get_accName(i); } catch {}
      try { role = acc.get_accRole(i); } catch {}
      try { action = acc.get_accDefaultAction(i); } catch {}
      try { acc.accLocation(out l, out t, out w, out ht, i); } catch {}
      o.Add(string.Format("CHILD\t{0}\t{1}\t{2}\t{3},{4},{5},{6}", name, role, action, l, t, w, ht));
    }
    return o;
  }
}
"@ -ReferencedAssemblies "Accessibility"

# ---------------------------------------------------------------- can we run?

if ([Environment]::UserInteractive -eq $false) {
    Write-Host "SKIP: no interactive desktop (a runner started as a service has none)."
    exit 2
}
if (Get-Process FlexTaskbar -ErrorAction SilentlyContinue) {
    Write-Host "SKIP: FlexTaskbar is already running; stop it first (FlexTaskbar.exe --exit)."
    exit 2
}
$Exe = (Resolve-Path $Exe -ErrorAction SilentlyContinue)
if (-not $Exe) {
    Write-Host "SKIP: no FlexTaskbar.exe to test. Build it first: cargo build --release"
    exit 2
}

# A copy with its own data folder, so an installed FlexTaskbar is untouched.
$dir = Join-Path ([System.IO.Path]::GetTempPath()) ("flextaskbar-smoke-" + [Guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $dir | Out-Null
$copy = Join-Path $dir "FlexTaskbar.exe"
Copy-Item $Exe $copy

$before = [Smoke]::WorkArea()
Write-Host "FlexTaskbar smoke test"
Write-Host "  exe:       $Exe"
Write-Host "  sandbox:   $dir"
Write-Host "  work area: $($before.Left),$($before.Top) - $($before.Right),$($before.Bottom)"
Write-Host ""

try {
    Start-Process -FilePath $copy -ArgumentList "--no-supervisor" | Out-Null

    # The first run scans the Start Menu, so give it time to put a bar up.
    $bar = $null
    $deadline = (Get-Date).AddSeconds($StartSeconds)
    while ((Get-Date) -lt $deadline) {
        $bars = [Smoke]::Windows("FlexTaskbar.Strip")
        if ($bars.Count -gt 0) { $bar = $bars[0]; break }
        Start-Sleep -Milliseconds 300
    }

    Check "the bar appears" ($null -ne $bar) "no FlexTaskbar.Strip window after $StartSeconds s"
    if (-not $bar) { throw "nothing to check without a bar" }

    $r = [Smoke]::Rect($bar)
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    Check "the bar has a sensible size" ($w -gt 100 -and $h -gt 10) "${w}x${h}"

    # Docked against a screen edge, not floating in the middle.
    $screen = [Smoke]::WorkArea()
    $touches = ($r.Left -le $screen.Left + 2) -or ($r.Top -le $screen.Top + 2) -or
               ($r.Right -ge $screen.Right - 2) -or ($r.Bottom -ge $screen.Bottom - 2)
    Check "the bar is docked against an edge" $touches "rect $($r.Left),$($r.Top) - $($r.Right),$($r.Bottom)"

    # Reserving space is what makes maximized windows stop above it.
    $after = [Smoke]::WorkArea()
    $shrank = ($after.Bottom - $after.Top) -lt ($before.Bottom - $before.Top) -or
              ($after.Right - $after.Left) -lt ($before.Right - $before.Left)
    Check "it reserves its screen space (AppBar)" $shrank `
        "work area was $($before.Bottom - $before.Top) tall, now $($after.Bottom - $after.Top)"

    # What a screen reader reads: a toolbar of named, pressable buttons.
    $acc = [Smoke]::Accessible($bar)
    $root = $acc | Where-Object { $_ -like "ROOT*" } | Select-Object -First 1
    $kids = @($acc | Where-Object { $_ -like "CHILD*" })
    $rootParts = $root -split "`t"
    Check "a screen reader sees the bar as a toolbar" ($rootParts[2] -eq 22) "role $($rootParts[2]), expected 22 (ROLE_SYSTEM_TOOLBAR)"
    Check "it has a name" ($rootParts[1] -eq "FlexTaskbar") "name '$($rootParts[1])'"
    Check "its buttons are reported" ($kids.Count -ge 3) "$($kids.Count) children"

    $named = @($kids | Where-Object { ($_ -split "`t")[1].Trim() })
    Check "every button has a name" ($named.Count -eq $kids.Count) "$($named.Count) of $($kids.Count) named"

    $placed = @($kids | Where-Object {
        $p = ($_ -split "`t")[4] -split ','
        [int]$p[2] -gt 0 -and [int]$p[3] -gt 0
    })
    Check "every button has a place on screen" ($placed.Count -eq $kids.Count) "$($placed.Count) of $($kids.Count) placed"

    $pressable = @($kids | Where-Object { ($_ -split "`t")[3].Trim() })
    Check "every button can be pressed" ($pressable.Count -eq $kids.Count) "$($pressable.Count) of $($kids.Count) have a default action"

    $allButton = @($kids | Where-Object { ($_ -split "`t")[1] -like "All*" })
    Check "the All button is there" ($allButton.Count -ge 1)

    Write-Host ""
    Write-Host "  what the screen reader reads:"
    foreach ($k in $kids) {
        $p = $k -split "`t"
        Write-Host "    '$($p[1])' role=$($p[2]) action='$($p[3])'"
    }

    # The bar hands the screen space back when it goes.
    & $copy --exit | Out-Null
    Start-Sleep -Seconds 3
    $returned = [Smoke]::WorkArea()
    Check "it gives the screen space back when it exits" `
        (($returned.Bottom - $returned.Top) -ge ($after.Bottom - $after.Top)) `
        "work area still $($returned.Bottom - $returned.Top) tall"
    Check "it leaves no crash log" (-not (Test-Path (Join-Path $dir "FlexTaskbarData\crash.log")))
}
finally {
    & $copy --exit 2>$null | Out-Null
    Start-Sleep -Seconds 2
    Get-Process FlexTaskbar -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $copy } | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:failures.Count -eq 0) {
    Write-Host "$($script:checks) checks, all passed."
    exit 0
}
Write-Host "$($script:checks) checks, $($script:failures.Count) failed:"
foreach ($f in $script:failures) { Write-Host "  - $f" }
exit 1
