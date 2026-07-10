use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::console;
use crate::kernel::process;
use crate::saifs;
use crate::shell;

const DEFAULT_INIT_SCRIPT: &str = "/system/init";
const DEFAULT_LOGIN_SHELL: &str = "/bin/busybox";
const DEFAULT_ROOT_USER: &str = "root";
const DEFAULT_ROOT_PASSWORD: &str = "root";
const DEFAULT_ROOT_HOME: &str = "/root";

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
        out.push(default_root_account(cfg));
    }
    out
}

fn default_root_account(cfg: &InitConfig) -> Account {
    Account {
        username: cfg.root_user.clone(),
        password: cfg.root_password.clone(),
        uid: 0,
        gid: 0,
        role: "superuser".to_string(),
        home: DEFAULT_ROOT_HOME.to_string(),
        shell: DEFAULT_LOGIN_SHELL.to_string(),
    }
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
            "{}:{}:0:0:superuser:{}:{}\n",
            DEFAULT_ROOT_USER,
            DEFAULT_ROOT_PASSWORD,
            DEFAULT_ROOT_HOME,
            DEFAULT_LOGIN_SHELL,
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

fn ensure_user_home_files(home: &str) {
    let home_path = if home.trim().is_empty() { "/root" } else { home };
    let _ = crate::saifs::mkdir(home_path);
    let bashrc = if home_path == "/" {
        "/.bashrc".to_string()
    } else {
        alloc::format!("{}/.bashrc", home_path.trim_end_matches('/'))
    };
    if crate::saifs::read_text(bashrc.as_str()).is_err() {
        let _ = crate::saifs::touch(bashrc.as_str());
        let _ = crate::vfs::write_path(
            bashrc.as_str(),
            b"# SAIOS shell startup\nalias ll ls\n",
        );
    }
}

fn read_runtime_state() -> RuntimeState {
    ensure_default_init_files();

    let cfg_text = saifs::read_text("/etc/init.conf").unwrap_or_default();
    let cfg = parse_init_config(cfg_text.as_str());

    let passwd = saifs::read_text("/etc/passwd").unwrap_or_default();
    let mut accounts = parse_passwd(passwd.as_str(), &cfg);

    if !accounts.iter().any(|a| a.username == cfg.root_user) {
        accounts.push(default_root_account(&cfg));
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

fn login_shell_args(shell: &str) -> &'static [&'static str] {
    if shell.eq_ignore_ascii_case("ash")
        || shell.eq_ignore_ascii_case("sh")
        || shell.eq_ignore_ascii_case("busybox")
        || shell.eq_ignore_ascii_case("/bin/busybox")
    {
        &["ash"]
    } else {
        &[]
    }
}

fn is_busybox_shell(shell: &str) -> bool {
    shell.eq_ignore_ascii_case("busybox") || shell.eq_ignore_ascii_case("/bin/busybox")
}

fn selected_account<'a>(state: &'a RuntimeState, username: &str) -> Option<&'a Account> {
    state.accounts.iter().find(|a| a.username == username)
}

fn account_home(account: Option<&Account>) -> String {
    account
        .map(|a| a.home.as_str())
        .filter(|home| !home.trim().is_empty())
        .unwrap_or(DEFAULT_ROOT_HOME)
        .to_string()
}

fn account_shell(account: Option<&Account>, config: &InitConfig) -> String {
    account
        .map(|a| a.shell.as_str())
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or(config.login_shell.as_str())
        .to_string()
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

    let _ = process::start_pid1("/system/init");

    if !state.config.hostname.is_empty() {
        let _ = crate::vfs::write_path(
            "/etc/hostname",
            alloc::format!("{}\n", state.config.hostname).as_bytes(),
        );
    }

    if let Err(e) = shell::run_init_script(state.config.init_script.as_str()) {
        console::println!("init: script failed: {}", e);
        console::println!("init: entering emergency shell");
        ensure_user_home_files("/root");
        let _ = crate::saifs::cd("/root");
        shell::run_shell_session(state.config.root_user.as_str(), None);
    }

    console::println!("init: ready (hostname={})", state.config.hostname);

    loop {
        let username;
        let user_home;
        let user_shell;
        loop {
            let user = prompt_line("login: ");
            let pass = prompt_line("password: ");
            if authenticate(&state, user.as_str(), pass.as_str()) {
                let account = selected_account(&state, user.as_str());
                user_home = account_home(account);
                user_shell = account_shell(account, &state.config);
                username = user;
                break;
            }
            console::println!("login: authentication failed");
        }

        ensure_user_home_files(user_home.as_str());
        let _ = crate::saifs::cd(user_home.as_str());

        let shell_name = user_shell.as_str();
        let shell_pid = process::ensure_shell_process(shell_name);
        let _ = process::create_session(shell_pid);
        let _ = process::set_foreground_process_group(shell_pid);
        let candidate_args = login_shell_args(shell_name);
        if shell_name.contains('/') {
            if let Ok(meta) = crate::shell::programs::binary_metadata_checked(shell_name) {
                if let Some(interp) = meta.interpreter.as_ref() {
                    console::println!(
                        "session: ring3 shell '{}' deferred (PT_INTERP='{}' not yet supported)",
                        shell_name,
                        interp
                    );
                    console::println!(
                        "session: ring3 shell launch failed (fallback to kernel SNSH)"
                    );
                    shell::run_shell_session(username.as_str(), None);
                }
            }
        }

        let exec_result = if is_busybox_shell(shell_name) {
            process::exec_path_in_place(shell_pid, shell_name, "ash", &[], &[])
                .map_err(|_| -22)
        } else {
            process::exec_in_place(shell_pid, shell_name, candidate_args, &[])
                .map_err(|_| -22)
        };

        match exec_result {
            Ok(code) => {
                console::println!(
                    "session: ring3 shell '{}' exited code={}",
                    shell_name,
                    code
                );
                console::println!("session: returning to login prompt");
            }
            Err(errno) => {
                console::println!(
                    "session: ring3 shell '{}' failed errno={}",
                    shell_name,
                    errno
                );
                console::println!(
                    "session: ring3 shell launch failed (last errno={}) (fallback to kernel SNSH)",
                    errno
                );
                shell::run_shell_session(username.as_str(), None);
            }
        }
    }
}
