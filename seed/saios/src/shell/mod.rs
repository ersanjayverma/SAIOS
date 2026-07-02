mod command;
mod commands;
mod compatibility;
mod dispatcher;
mod engine;
mod parser;
mod prompt;
pub mod programs;
mod registry;
mod service;
mod session;
mod native;

use crate::console;
use crate::kernel::package_image;
use crate::kernel::object as kom;
use crate::object_manager;
use crate::saifs;

pub fn init() {
    console::clear();
    console::println!("SAIOS v0.10");
    console::println!("UEFI Boot");
    console::println!("Launching SISH...");
    console::println!("UTF framebuffer: Cafe Ω α あ ┌─┐ █");
    console::newline();
    object_manager::init();
    saifs::init();
    let _ = package_image::mount_default();
    kom::init();
}

pub fn run() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

pub fn start_service() -> Result<(), &'static str> {
    service::start()
}
