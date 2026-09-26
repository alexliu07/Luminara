#[cfg(windows)]
mod app;
mod model;
mod platform;

#[cfg(windows)]
use windows_reactor::*;

#[cfg(windows)]
use crate::app::root::Root;

#[cfg(windows)]
fn main() {
    App::run_component::<Root>(()).unwrap();
}

/// 本应用仅支持 Windows;其余平台只编译模型层与字幕引擎(供单测)。
#[cfg(not(windows))]
fn main() {
    eprintln!("Luminara 仅支持 Windows");
}
