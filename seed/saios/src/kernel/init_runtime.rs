use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::console;
use crate::kernel::process;
use crate::saifs;
use crate::shell;

const DEFAULT_INIT_SCRIPT: &str = "/system/init";
const DEFAULT_LOGIN_SHELL: &str = "snsh";
const DEFAULT_ROOT_USER: &str = "root";
const DEFAULT_ROOT_PASSWORD: &str = "root";

#[derive(Clone, Debug)]
pub struct UserSummary {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub role: String,
    pub home: String,
    pub shell: String,
}

#[derive(Clone, Debug)]
struct Account {
    username: String,
    password: String,
    uid: u32,
    gid: u32,
    role: String,
    home: String,
    shell: String,
}

#[derive(Clone, Debug)]
struct InitConfig {
    hostname: String,
    init_script: String,
    login_shell: String,
    root_user: String,
    root_password: String,
}

#[derive(Clone, Debug)]
struct RuntimeState {
    config: InitConfig,
    accounts: Vec<Account>,
}

static INIT_LOCK: AtomicBool = AtomicBool::new(false);
static INIT_STATE: StaticCell<Option<RuntimeState>> = StaticCell::new(None);

fn lock() {
    while INIT_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    INIT_LOCK.store(false, Ordering::Release);
}

fn with_state<R>(f: impl FnOnce(&mut Option<RuntimeState>) -> R) -> R {
    lock();
    // SAFETY: singleton guarded by spin lock.
    let out = unsafe {
        let slot = &mut *INIT_STATE.get();
        f(slot)
    };
    unlock();
    out
}

fn default_config() -> InitConfig {
    InitConfig {
        hostname: "saios".to_string(),
        init_script: DEFAULT_INIT_SCRIPT.to_string(),
        login_shell: DEFAULT_LOGIN_SHELL.to_string(),
        root_user: DEFAULT_ROOT_USER.to_string(),
        root_password: DEFAULT_ROOT_PASSWORD.to_string(),
    }
}

fn parse_init_config(text: &str) -> InitConfig {
    let mut cfg = default_config();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim();
        if value.is_empty() {
            continue;
        }

        if key.eq_ignore_ascii_case("hostname") {
            cfg.hostname = value.to_string();
        } else if key.eq_ignore_ascii_case("init_script") {
            cfg.init_script = value.to_string();
        } else if key.eq_ignore_ascii_case("login_shell") {
            cfg.login_shell = value.to_string();
        } else if key.eq_ignore_ascii_case("root_user") {
            cfg.root_user = value.to_string();
        } else if key.eq_ignore_ascii_case("root_password") {
            cfg.root_password = value.to_string();
        }
    }
    cfg
}

fn parse_passwd(text: &str, cfg: &InitConfig) -> Vec<Account> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 {
            continue;
        }

        let uid = parts[2].parse::<u32>().ok().unwrap_or(0);
        let gid = parts[3].parse::<u32>().ok().unwrap_or(uid);
        out.push(Account {
            username: parts[0].to_string(),
            password: parts[1].to_string(),
            uid,
            gid,
            role: parts[4].to_string(),
            home: parts[5].to_string(),
            shell: parts[6].to_string(),
        });
    }

    if out.is_empty() {
        out.push(Account {
            username: cfg.root_user.clone(),
            password: cfg.root_password.clone(),
            uid: 0,
            gid: 0,
            role: "superuser".to_string(),
            home: "/root".to_string(),
            shell: "/bin/shell".to_string(),
        });
    }
    out
}

fn ensure_default_init_files() {
    let _ = vfs_touch("/etc/init.conf");
    if saifs::read_text("/etc/init.conf").is_err() {
        let default_text = alloc::format!(
            "# SAIOS init configuration\nhostname=saios\ninit_script={}\nlogin_shell={}\nroot_user={}\nroot_password={}\n",
            DEFAULT_INIT_SCRIPT,
            DEFAULT_LOGIN_SHELL,
            DEFAULT_ROOT_USER,
            DEFAULT_ROOT_PASSWORD
        );
        let _ = crate::vfs::write_path("/etc/init.conf", default_text.as_bytes());
    }

    let _ = vfs_touch("/etc/passwd");
    if saifs::read_text("/etc/passwd").is_err() {
        let default_passwd = alloc::format!(
            "{}:{}:0:0:superuser:/root:/bin/shell\n",
            DEFAULT_ROOT_USER,
            DEFAULT_ROOT_PASSWORD
        );
        let _ = crate::vfs::write_path("/etc/passwd", default_passwd.as_bytes());
    }
}

fn vfs_touch(path: &str) -> Result<(), &'static str> {
    match crate::vfs::touch(path) {
        Ok(()) => Ok(()),
        Err("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

fn read_runtime_state() -> RuntimeState {
    ensure_default_init_files();

    let cfg_text = saifs::read_text("/etc/init.conf").unwrap_or_default();
    let cfg = parse_init_config(cfg_text.as_str());

    let passwd = saifs::read_text("/etc/passwd").unwrap_or_default();
    let mut accounts = parse_passwd(passwd.as_str(), &cfg);

    if !accounts.iter().any(|a| a.username == cfg.root_user) {
        accounts.push(Account {
            username: cfg.root_user.clone(),
            password: cfg.root_password.clone(),
            uid: 0,
            gid: 0,
            role: "superuser".to_string(),
            home: "/root".to_string(),
            shell: "/bin/shell".to_string(),
        });
    }

    RuntimeState {
        config: cfg,
        accounts,
    }
}

fn load_runtime_state() -> RuntimeState {
    with_state(|slot| {
        if slot.is_none() {
            *slot = Some(read_runtime_state());
        }
        slot.clone().unwrap_or_else(read_runtime_state)
    })
}

fn authenticate(state: &RuntimeState, username: &str, password: &str) -> bool {
    state
        .accounts
        .iter()
        .any(|a| a.username == username && a.password == password)
}

fn prompt_line(prompt: &str) -> String {
    console::set_input_prompt(prompt);
    console::print(prompt);
    console::read_line().as_str().trim().to_string()
}

pub fn current_config_summary() -> Option<(String, String, String)> {
    with_state(|slot| {
        slot.as_ref().map(|s| {
            (
                s.config.hostname.clone(),
                s.config.init_script.clone(),
                s.config.login_shell.clone(),
            )
        })
    })
}

pub fn user_summary(username: &str) -> Option<UserSummary> {
    let state = load_runtime_state();
    state
        .accounts
        .iter()
        .find(|a| a.username == username)
        .map(|a| UserSummary {
            username: a.username.clone(),
            uid: a.uid,
            gid: a.gid,
            role: a.role.clone(),
            home: a.home.clone(),
            shell: a.shell.clone(),
        })
}

pub fn boot_to_login_shell() -> ! {
    let state = load_runtime_state();

    let _ = process::start_pid1("/sbin/init");

    if !state.config.hostname.is_empty() {
        let _ = crate::vfs::write_path(
            "/etc/hostname",
            alloc::format!("{}\n", state.config.hostname).as_bytes(),
        );
    }

    shell::run_init_script(state.config.init_script.as_str());

    console::println!("init: ready (hostname={})", state.config.hostname);

    let username;
    loop {
        let user = prompt_line("login: ");
        let pass = prompt_line("password: ");
        if authenticate(&state, user.as_str(), pass.as_str()) {
            username = user;
            break;
        }
        console::println!("login: authentication failed");
    }

    let shell_pid = process::ensure_shell_process(state.config.login_shell.as_str());
    let _ = process::create_session(shell_pid);
    let _ = process::set_foreground_process_group(shell_pid);

    shell::run_shell_session(username.as_str(), Some(state.config.init_script.as_str()));
}
