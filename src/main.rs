mod app;
mod model;

use windows_reactor::*;

use crate::app::root::Root;

fn main() {
    App::run_component::<Root>(()).unwrap();
}
