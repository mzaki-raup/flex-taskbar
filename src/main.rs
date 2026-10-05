#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod allview;
mod anim;
mod appearance;
mod appkind;
mod config;
mod flyanim;
mod migrate;
mod pkgsources;
mod search;
mod shadow;
mod striplayout;
mod tree;

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    std::process::exit(win::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("FlexTaskbar runs on Windows only. On other platforms only `cargo test` is useful.");
    std::process::exit(1);
}
