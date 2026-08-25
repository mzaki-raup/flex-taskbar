# Installation

## Requirements

- Windows 10 22H2 or later, or Windows 11.
- No administrator privileges — installation is per-user.
- To **build** the installer yourself: .NET 8+ SDK (`dotnet build` restores the WiX
  toolset automatically via NuGet — no separate WiX installation needed).

## Installing (from a built MSI)

```powershell
msiexec /i FlexTaskbar.Installer.msi
```

or just double-click the `.msi` file. This opens a standard feature-selection
installer (`WixUI_FeatureTree`) with three independently-selectable features:

- **FlexTaskbar** (required) — the app itself, installed to
  `%LocalAppData%\Programs\FlexTaskbar`, plus a Start Menu shortcut.
- **Desktop shortcut** (on by default) — adds a shortcut to your desktop.
- **Start with Windows** (**off** by default — Section 2's safety-by-default rule
  applies here too) — adds a shortcut to your Startup folder so FlexTaskbar launches
  automatically at sign-in. This is the exact same shortcut FlexTaskbar's own
  Settings → General → "Start with Windows" checkbox manages at runtime, so the two
  can never disagree with each other.

Uncheck any optional feature in the installer's feature tree before clicking
Install if you don't want it.

### Silent install

```powershell
msiexec /i FlexTaskbar.Installer.msi /qn
```

Installs with default feature selections (app + Start Menu + Desktop shortcut;
Startup shortcut stays off, matching the interactive default).

## Uninstalling

```powershell
msiexec /x FlexTaskbar.Installer.msi
```

Removes the installed files and all shortcuts (Start Menu, Desktop, and Startup —
whichever were installed). **Never touches `%APPDATA%\FlexTaskbar`** (your
categories, settings, and cached application list) — that's created by the app at
runtime, not by the installer, so uninstalling the app doesn't know it exists and
can't accidentally delete it. Delete that folder yourself if you want a completely
clean removal.

> **Known limitation**: the installed product does not currently appear in Windows'
> "Programs and Features" / Add-or-Remove-Programs list, even though installation
> completes successfully and installs/uninstalls correctly via `msiexec`. The
> underlying MSI product registration (`RegisterProduct`/`PublishProduct`) does
> succeed — this is specifically about the Control Panel *display* entry. Use the
> `msiexec /x` command above, or re-run the original `.msi` (which offers
> repair/uninstall for an already-installed product), until this is fixed.

## Building the installer yourself

```bash
dotnet build installer/FlexTaskbar.Installer.wixproj -c Release
```

Produces `installer/bin/Release/FlexTaskbar.Installer.msi` (or `bin/x64/Release/...`
depending on platform). The installer project references the main app project
directly (`ProjectReference`), so it always packages whatever the app project
actually built — no hand-maintained file list to keep in sync.

Building the whole solution (`dotnet build FlexTaskbar.sln -c Release`) builds the
app, tests, and installer together.

## Build & run from source (no installer)

```bash
git clone <this-repo>
cd flex-taskbar
dotnet build FlexTaskbar.sln -c Release
dotnet run --project src/FlexTaskbar/FlexTaskbar.csproj
```

Nothing is installed system-wide this way — just delete the repo checkout to remove
it. If you manually created a Startup-folder shortcut (via FlexTaskbar's own
Settings, not the installer), remove it from `shell:startup`, or use FlexTaskbar's
Settings checkbox / `FlexTaskbar.exe --disable` to remove it properly.
