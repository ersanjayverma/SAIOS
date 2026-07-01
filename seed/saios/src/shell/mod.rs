mod command;
mod compatibility;
mod engine;
mod parser;
mod registry;
mod session;
mod native;

use crate::console;
use crate::object_manager;
use crate::saifs;

pub fn init() {
    console::clear();
    object_manager::init();
    saifs::init();
}

pub fn run() -> ! {
    let mut engine = engine::ShellEngine::new();
    engine.run()
}
