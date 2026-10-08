use std::io::Write;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/app.ico");
    // The version block below is built from the package version, so a release
    // that changes only Cargo.toml still rebuilds the resource.
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    // Only Windows builds get the icon + manifest resource. The manifest is not
    // optional: it opts into Common Controls v6 (visual styles, alpha image lists,
    // SetWindowSubclass) and per-monitor-v2 DPI awareness.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let rc = write_resource_script();
        embed_resource::compile(rc, embed_resource::NONE).manifest_required().unwrap();
    }
}

/// Writes the resource script the build actually compiles: `assets/app.rc`
/// plus a `VERSIONINFO` block, so Explorer's *Details* tab and the file
/// properties dialog show the version the app was built with.
///
/// It is generated rather than kept beside the others because the version has
/// to come from `Cargo.toml` — a hand-written one would be a fifth place to
/// remember at release time, and the first to be forgotten.
fn write_resource_script() -> std::path::PathBuf {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let assets =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("assets");
    // The script sits in OUT_DIR, so the files it names need full paths.
    // Forward slashes: the resource compiler takes them, and they need no
    // escaping inside a string.
    let dir = assets.display().to_string().replace('\\', "/");

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let description = std::env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();
    // FILEVERSION wants four numbers; a package version is three.
    let mut parts: Vec<u16> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let (major, minor, patch, build) = (parts[0], parts[1], parts[2], parts[3]);

    let script = format!(
        r##"#define RT_MANIFEST 24

1 ICON "{dir}/app.ico"
1 RT_MANIFEST "{dir}/app.manifest"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},{build}
PRODUCTVERSION {major},{minor},{patch},{build}
FILEOS 0x4L
FILETYPE 0x1L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{major}.{minor}.{patch}.{build}"
            VALUE "InternalName", "FlexTaskbar"
            VALUE "OriginalFilename", "FlexTaskbar.exe"
            VALUE "ProductName", "FlexTaskbar"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        /* US English, Unicode. */
        VALUE "Translation", 0x409, 1200
    END
END
"##
    );

    let path = out.join("app-with-version.rc");
    let mut f = std::fs::File::create(&path).expect("writing the resource script");
    f.write_all(script.as_bytes()).expect("writing the resource script");
    path
}
