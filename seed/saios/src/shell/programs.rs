use crate::console;
use alloc::string::String;

type ProgramResult = Result<i32, &'static str>;

fn hello_program(args: &[&str], env: &[(String, String)]) -> i32 {
    console::println!("hello from SAIOS demo program");
    if !args.is_empty() {
        console::println!("args: {}", args.join(" "));
    }
    console::println!("env vars: {}", env.len());
    0
}

fn true_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    0
}

fn false_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    1
}

fn argc_program(args: &[&str], _env: &[(String, String)]) -> i32 {
    console::println!("argc={}", args.len());
    args.len() as i32
}

fn env_program(_args: &[&str], env: &[(String, String)]) -> i32 {
    for (k, v) in env {
        console::println!("{}={}", k, v);
    }
    0
}

fn fail_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let code = args
        .first()
        .and_then(|raw| raw.parse::<i32>().ok())
        .unwrap_or(1);
    Ok(code)
}

pub fn execute(name: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    match name {
        n if n.eq_ignore_ascii_case("hello") => Ok(hello_program(args, env)),
        n if n.eq_ignore_ascii_case("true") => Ok(true_program(args, env)),
        n if n.eq_ignore_ascii_case("false") => Ok(false_program(args, env)),
        n if n.eq_ignore_ascii_case("argc") => Ok(argc_program(args, env)),
        n if n.eq_ignore_ascii_case("env") => Ok(env_program(args, env)),
        n if n.eq_ignore_ascii_case("fail") => fail_program(args, env),
        _ => Err("program not found"),
    }
}

