#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod anim;
mod appearance;
mod config;
mod migrate;
mod search;
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
