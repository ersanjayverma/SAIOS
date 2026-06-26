use alloc::format;
use alloc::string::String;

pub const NATIVE_ROOTS: &[&str] = crate::saios::rootfs::AUTHORITATIVE_ROOTS;

pub const NATIVE_SYSTEM_DIRS: &[&str] = &[
    "/system/config",
    "/system/config/system",
    "/system/config/network",
    "/system/config/display",
    "/system/config/security",
    "/system/config/packages",
    "/system/config/ai",
    "/system/users",
    "/system/auth",
    "/system/auth/acls",
    "/system/auth/capabilities",
    "/system/auth/services",
    "/system/services",
    "/system/compat",
    "/system/compat/windows",
    "/system/compat/macos",
];

pub const NATIVE_SUPPORT_DIRS: &[&str] = &["/users/root"];

pub const COMPAT_VIEW_ROOTS: &[&str] = &["/linux", "/windows", "/macos"];

pub const LINUX_COMPAT_ROOT: &str = "/";
pub const WINDOWS_COMPAT_ROOT: &str = "/system/compat/windows";
pub const MACOS_COMPAT_ROOT: &str = "/system/compat/macos";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceView {
    Native,
    Linux,
    Windows,
    MacOs,
}

pub fn current_view() -> NamespaceView {
    crate::process::with_current_process(|p| p.namespace_view).unwrap_or(NamespaceView::Native)
}

pub fn translate_path(path: &str) -> String {
    translate_path_for_view(path, current_view())
}

pub fn translate_path_for_view(path: &str, view: NamespaceView) -> String {
    if path.is_empty() {
        return String::from("/");
    }

    if let Some(mapped) = translate_explicit_view_root(path) {
        return mapped;
    }

    if let Some(mapped) = translate_windows_drive_path(path) {
        return mapped;
    }

    match view {
        NamespaceView::Native => String::from(path),
        NamespaceView::Linux => translate_linux_path(path),
        NamespaceView::Windows => translate_windows_path(path),
        NamespaceView::MacOs => translate_macos_path(path),
    }
}

fn translate_linux_path(path: &str) -> String {
    String::from(path)
}

fn translate_windows_path(path: &str) -> String {
    if path == "/" {
        return String::from("/system/compat/windows/C");
    }
    if path.starts_with('\\') {
        let trimmed = path.trim_start_matches('\\').replace('\\', "/");
        return format!(
            "/system/compat/windows/C/{}",
            trimmed.trim_start_matches('/')
        );
    }
    if should_preserve_native_path(path) || path.starts_with("/system/compat/") {
        return String::from(path);
    }
    if path.starts_with('/') {
        return format!("/system/compat/windows/C{}", path);
    }
    String::from(path)
}

fn translate_macos_path(path: &str) -> String {
    if path == "/" {
        return String::from(MACOS_COMPAT_ROOT);
    }
    if should_preserve_native_path(path) || path.starts_with("/system/compat/") {
        return String::from(path);
    }
    if path.starts_with('/') {
        return format!("{}{}", MACOS_COMPAT_ROOT, path);
    }
    String::from(path)
}

fn translate_explicit_view_root(path: &str) -> Option<String> {
    translate_prefix(path, "/linux", LINUX_COMPAT_ROOT)
        .or_else(|| translate_prefix(path, "/windows", WINDOWS_COMPAT_ROOT))
        .or_else(|| translate_prefix(path, "/macos", MACOS_COMPAT_ROOT))
}

fn translate_prefix(path: &str, prefix: &str, target_root: &str) -> Option<String> {
    if path == prefix {
        return Some(String::from(target_root));
    }
    path.strip_prefix(prefix)
        .filter(|suffix| suffix.starts_with('/'))
        .map(|suffix| {
            if target_root == "/" {
                String::from(suffix)
            } else {
                format!("{}{}", target_root, suffix)
            }
        })
}

fn translate_windows_drive_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }

    let drive = (bytes[0] as char).to_ascii_uppercase();
    let suffix = path[2..].replace('\\', "/");
    let suffix = suffix.trim_start_matches('/');
    if suffix.is_empty() {
        Some(format!("{}/{drive}", WINDOWS_COMPAT_ROOT))
    } else {
        Some(format!("{}/{drive}/{}", WINDOWS_COMPAT_ROOT, suffix))
    }
}

fn should_preserve_native_path(path: &str) -> bool {
    NATIVE_ROOTS.iter().any(|root| matches_root(path, root))
        || NATIVE_SUPPORT_DIRS
            .iter()
            .any(|root| matches_root(path, root))
}

fn matches_root(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
