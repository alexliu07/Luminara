mod app;
mod model;
mod platform;

use windows_reactor::*;

use crate::app::root::Root;

fn main() {
    App::run_component::<Root>(()).unwrap();
}
