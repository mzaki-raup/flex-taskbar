fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/app.ico");

    // Only Windows builds get the icon + manifest resource. The manifest is not
    // optional: it opts into Common Controls v6 (visual styles, alpha image lists,
    // SetWindowSubclass) and per-monitor-v2 DPI awareness.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
