mod builtins;
mod commands;
mod parser;

use crate::console;

const PROMPT: &str = "SAIOS v0.1>";

pub fn init() {
    console::clear();
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
