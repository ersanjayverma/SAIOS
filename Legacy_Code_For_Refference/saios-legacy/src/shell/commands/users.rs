use crate::{print, println};
use alloc::string::{String, ToString};

fn switch_user(username: &str) -> Result<crate::user::User, &'static str> {
    let user = crate::user::get_user_by_name(username).ok_or("unknown user")?;
    let _ = crate::process::with_current_process_mut(|proc| {
        proc.uid = user.uid;
        proc.gid = user.gid;
        proc.euid = user.uid;
        proc.egid = user.gid;
        proc.suid = user.uid;
        proc.sgid = user.gid;
        proc.cwd = user.home.clone();
    });

    crate::shell::set_current_cwd(&user.home);

    let env_text = alloc::format!(
        "HOME={}\nUSER={}\nSHELL={}\nPWD={}\n",
        user.home,
        user.username,
        user.shell,
        user.home
    );
    let _ = super::filesystem::write_env_bytes(env_text.as_bytes());

    Ok(user)
}

fn reset_to_console_root() -> Result<crate::user::User, &'static str> {
    switch_user("root")
}

pub fn id() {
    let user = crate::user::get_current_user().unwrap_or_else(|| crate::user::User {
        uid: 0,
        gid: 0,
        username: String::from("root"),
        home: String::from("/users/root"),
        shell: String::from("/bin/sh"),
    });

    println!(
        "uid={}({}) gid={}({})",
        user.uid, user.username, user.gid, user.username
    );
}

pub fn whoami() {
    let user = crate::user::get_current_user().unwrap_or_else(|| crate::user::User {
        uid: 0,
        gid: 0,
        username: String::from("root"),
        home: String::from("/users/root"),
        shell: String::from("/bin/sh"),
    });

    println!("{}", user.username);
}

pub fn users() {
    let users = crate::user::get_all_users();
    for user in users {
        println!("{}", user.username);
    }
}

pub fn useradd(args: &str) {
    let username = args.trim();
    if username.is_empty() {
        println!("usage: useradd <username>");
        return;
    }

    match crate::user::add_user(username.to_string(), None) {
        Ok(uid) => {
            println!("User '{}' created with UID {}", username, uid);
        }
        Err(e) => {
            println!("useradd: {}", e);
        }
    }
}

pub fn userdel(args: &str) {
    let username = args.trim();
    if username.is_empty() {
        println!("usage: userdel <username>");
        return;
    }

    println!(
        "userdel: deleting user '{}' - not yet implemented",
        username
    );
}

pub fn login(args: &str) {
    let username = args.trim();
    if username.is_empty() {
        println!("usage: login <username>");
        return;
    }

    match switch_user(username) {
        Ok(user) => println!("logged in as {}", user.username),
        Err(e) => println!("login: {}", e),
    }
}

pub fn su(args: &str) {
    let username = if args.trim().is_empty() {
        "root"
    } else {
        args.trim()
    };

    match switch_user(username) {
        Ok(user) => println!("switched to {}", user.username),
        Err(e) => println!("su: {}", e),
    }
}

pub fn passwd(args: &str) {
    let username = if args.trim().is_empty() {
        crate::user::get_current_user()
            .map(|user| user.username)
            .unwrap_or_else(|| String::from("root"))
    } else {
        args.trim().to_string()
    };

    if crate::user::get_user_by_name(&username).is_none() {
        println!("passwd: unknown user '{}';", username);
        return;
    }

    println!(
        "passwd: password authentication is not implemented; '{}' remains managed through locked shadow entries",
        username
    );
}

pub fn logout() {
    match reset_to_console_root() {
        Ok(user) => println!("logged out to {}", user.username),
        Err(e) => println!("logout: {}", e),
    }
}

pub fn env_cmd(_args: &str) {
    match super::filesystem::cat_read_env() {
        Ok(text) => {
            if text.trim().is_empty() {
                println!("(no variables set)");
            } else {
                print!("{}", text);
            }
        }
        Err(_) => println!("(no variables set)"),
    }
}

pub fn set_cmd(args: &str) {
    let mut p = args.splitn(2, ' ');
    let key = p.next().unwrap_or("").trim();
    let val = p.next().unwrap_or("").trim();
    if key.is_empty() {
        println!("usage: set <key> <value>");
        return;
    }
    let line = alloc::format!("{}={}\n", key, val);
    let mut v = super::filesystem::read_env_bytes();
    v.extend_from_slice(line.as_bytes());
    let _ = super::filesystem::write_env_bytes(&v);
    println!("{}={}", key, val);
}

pub fn todo_cmd(args: &str) {
    if args.is_empty() {
        println!("usage: todo <text>");
        return;
    }
    let line = alloc::format!("- {}\n", args);
    let mut v = super::filesystem::read_todo_bytes();
    v.extend_from_slice(line.as_bytes());
    let _ = super::filesystem::write_todo_bytes(&v);
    println!("Added: {}", args);
}

pub fn notes() {
    super::filesystem::cat("/home/todo.txt");
}

pub fn help_users() {
    println!("  User Management:");
    println!("    id                 show current user and group IDs");
    println!("    whoami             show current username");
    println!("    users              list all users");
    println!("    login <name>       switch the console session to a user");
    println!("    logout             return the console session to root");
    println!("    su [name]          switch effective user (defaults to root)");
    println!("    passwd [name]      show current password-management status");
    println!("    useradd <name>     create a new user");
    println!("    userdel <name>     delete a user");
}
