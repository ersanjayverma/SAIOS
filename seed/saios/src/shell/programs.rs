use crate::console;
use crate::scheduler;

fn hello_program() {
    console::println!("hello from SAIOS demo program");
    scheduler::yield_now();
}

pub fn launch(name: &str) -> Result<(), &'static str> {
    match name {
        n if n.eq_ignore_ascii_case("hello") => {
            let _ = scheduler::spawn(hello_program);
            Ok(())
        }
        _ => Err("program not found"),
    }
}
