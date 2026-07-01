mod builtins;
mod commands;
mod parser;

use crate::console;
use crate::object_manager;
use crate::vfs;

const PROMPT: &str = "SAIOS v0.1>";

pub fn init() {
    console::clear();
    object_manager::init();
    vfs::init();
}

pub fn run() -> ! {
    loop {
        console::print(PROMPT);
        let line = console::read_line();

        if let Some(parsed) = parser::parse_line(line.as_str()) {
            builtins::execute(parsed);
        }
    }
}
