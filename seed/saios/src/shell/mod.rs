mod command;
mod commands;
mod compatibility;
mod dispatcher;
mod engine;
mod parser;
mod prompt;
mod registry;
mod service;
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
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

pub fn start_service() -> Result<(), &'static str> {
    service::start()
}
