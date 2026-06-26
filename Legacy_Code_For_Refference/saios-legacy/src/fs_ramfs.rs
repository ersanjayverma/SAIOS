//! SAIOS RAM filesystem — a simple in-memory tree of files and directories.
//! Persists only until reboot; gives the shell a place to read/write data.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Debug)]
pub enum FsError {
    NotFound,
    IsDirectory,
    IsFile,
    NameTooLong,
    AlreadyExists,
    InvalidPath,
}

impl FsError {
    pub fn msg(&self) -> &'static str {
        match self {
            Self::NotFound => "no such file or directory",
            Self::IsDirectory => "is a directory",
            Self::IsFile => "is a file",
            Self::NameTooLong => "name too long",
            Self::AlreadyExists => "already exists",
            Self::InvalidPath => "invalid path",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Node {
    File(Vec<u8>),
    Dir(BTreeMap<String, Node>),
}

static ROOT: Mutex<Node> = Mutex::new(Node::Dir(BTreeMap::new()));

pub fn init() {
    let mut root = ROOT.lock();
    // Create standard top-level directories
    if let Node::Dir(entries) = &mut *root {
        for dir in &["bin", "etc", "home", "tmp", "ai"] {
            entries.insert(dir.to_string(), Node::Dir(BTreeMap::new()));
        }
        // /etc/saios.conf
        if let Some(Node::Dir(etc)) = entries.get_mut("etc") {
            etc.insert("saios.conf".to_string(), Node::File(
                b"# SAIOS configuration\nai_provider=ollama\nai_host=10.0.2.2:11434\nai_model=llama3\nollama_host=10.0.2.2\nollama_port=11434\ntogether_model=openai/gpt-oss-120b\n"
                    .to_vec()
            ));
        }
    }
    crate::serial_println!("[fs] ramfs mounted at /");
}

/// Read a file's contents as bytes.
pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    let root = ROOT.lock();
    match walk(&root, path)? {
        Node::File(data) => Ok(data.clone()),
        Node::Dir(_) => Err(FsError::IsDirectory),
    }
}

/// Write (create or overwrite) a file.
pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    let (dir_path, name) = split_path(path)?;
    let mut root = ROOT.lock();
    match walk_mut(&mut root, dir_path)? {
        Node::Dir(entries) => {
            entries.insert(name.to_string(), Node::File(data.to_vec()));
            Ok(())
        }
        Node::File(_) => Err(FsError::IsFile),
    }
}

/// Append bytes to a file (creates it if absent).
pub fn append(path: &str, data: &[u8]) -> Result<(), FsError> {
    let existing = read(path).unwrap_or_default();
    let mut buf = existing;
    buf.extend_from_slice(data);
    write(path, &buf)
}

/// List a directory.
pub fn ls(path: &str) -> Result<Vec<String>, FsError> {
    let root = ROOT.lock();
    match walk(&root, path)? {
        Node::Dir(entries) => Ok(entries.keys().cloned().collect()),
        Node::File(_) => Err(FsError::IsFile),
    }
}

/// Create a directory (and any missing parents).
pub fn mkdir(path: &str) -> Result<(), FsError> {
    let parts = path_parts(path);
    let mut root = ROOT.lock();
    let mut cur = &mut *root;
    for part in parts {
        if let Node::Dir(entries) = cur {
            cur = entries
                .entry(part.to_string())
                .or_insert(Node::Dir(BTreeMap::new()));
        } else {
            return Err(FsError::IsFile);
        }
    }
    Ok(())
}

/// Delete a file or empty directory.
pub fn remove(path: &str) -> Result<(), FsError> {
    let (dir_path, name) = split_path(path)?;
    let mut root = ROOT.lock();
    match walk_mut(&mut root, dir_path)? {
        Node::Dir(entries) => {
            entries.remove(name).ok_or(FsError::NotFound)?;
            Ok(())
        }
        Node::File(_) => Err(FsError::IsFile),
    }
}

/// Return disk usage summary: (files, dirs, total_bytes).
pub fn stat() -> (usize, usize, usize) {
    let root = ROOT.lock();
    count_recursive(&root)
}

fn count_recursive(node: &Node) -> (usize, usize, usize) {
    match node {
        Node::File(d) => (1, 0, d.len()),
        Node::Dir(entries) => entries.values().fold((0, 1, 0), |acc, n| {
            let (f, d, b) = count_recursive(n);
            (acc.0 + f, acc.1 + d, acc.2 + b)
        }),
    }
}

// -- Path helpers -----------------------------------------------------------

fn path_parts(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_path(path: &str) -> Result<(&str, &str), FsError> {
    let path = path.trim_end_matches('/');
    match path.rfind('/') {
        Some(i) => Ok((&path[..i.max(1)], &path[i + 1..])),
        None => Ok(("/", path)),
    }
}

fn walk<'a>(root: &'a Node, path: &str) -> Result<&'a Node, FsError> {
    let parts = path_parts(path);
    let mut cur = root;
    for part in parts {
        match cur {
            Node::Dir(entries) => {
                cur = entries.get(part).ok_or(FsError::NotFound)?;
            }
            Node::File(_) => return Err(FsError::IsFile),
        }
    }
    Ok(cur)
}

fn walk_mut<'a>(root: &'a mut Node, path: &str) -> Result<&'a mut Node, FsError> {
    let parts = path_parts(path);
    let mut cur = root;
    for part in parts {
        match cur {
            Node::Dir(entries) => {
                cur = entries.get_mut(part).ok_or(FsError::NotFound)?;
            }
            Node::File(_) => return Err(FsError::IsFile),
        }
    }
    Ok(cur)
}
