use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::object_manager;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileType {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub struct VNode {
    pub inode: u64,
    pub name: String,
    pub kind: FileType,
}

#[derive(Clone)]
struct Node {
    inode: u64,
    name: String,
    kind: FileType,
    parent: Option<u64>,
    children: Vec<u64>,
    data: Vec<u8>,
}

pub trait FileSystem {
    fn create(&mut self, path: &str) -> Result<(), &'static str>;
    fn open(&self, path: &str) -> Result<VNode, &'static str>;
    fn read(&self, path: &str) -> Result<Vec<u8>, &'static str>;
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str>;
    fn remove(&mut self, path: &str) -> Result<(), &'static str>;
    fn mkdir(&mut self, path: &str) -> Result<(), &'static str>;
    fn readdir(&self, path: &str) -> Result<Vec<String>, &'static str>;
}

struct TmpFs {
    nodes: Vec<Option<Node>>,
    root: u64,
    cwd: u64,
}

impl TmpFs {
    fn new() -> Self {
        let mut nodes = Vec::new();
        let root = Node {
            inode: 1,
            name: "/".to_string(),
            kind: FileType::Directory,
            parent: None,
            children: Vec::new(),
            data: Vec::new(),
        };
        nodes.push(Some(root));
        Self {
            nodes,
            root: 1,
            cwd: 1,
        }
    }

    fn inode_to_index(inode: u64) -> Option<usize> {
        inode.checked_sub(1).map(|v| v as usize)
    }

    fn node(&self, inode: u64) -> Result<&Node, &'static str> {
        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        self.nodes
            .get(idx)
            .and_then(|n| n.as_ref())
            .ok_or("node missing")
    }

    fn node_mut(&mut self, inode: u64) -> Result<&mut Node, &'static str> {
        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        self.nodes
            .get_mut(idx)
            .and_then(|n| n.as_mut())
            .ok_or("node missing")
    }

    fn next_inode(&self) -> u64 {
        (self.nodes.len() as u64) + 1
    }

    fn path_parts(path: &str) -> impl Iterator<Item = &str> {
        path.split('/').filter(|p| !p.is_empty())
    }

    fn lookup_child_by_name(&self, dir_inode: u64, name: &str) -> Option<u64> {
        let dir = self.node(dir_inode).ok()?;
        if dir.kind != FileType::Directory {
            return None;
        }

        for &child_inode in &dir.children {
            if let Ok(child) = self.node(child_inode) {
                if child.name == name {
                    return Some(child_inode);
                }
            }
        }
        None
    }

    fn resolve_from(&self, start: u64, path: &str) -> Result<u64, &'static str> {
        if path == "/" {
            return Ok(self.root);
        }

        let mut current = if path.starts_with('/') { self.root } else { start };

        for part in Self::path_parts(path) {
            match part {
                "." => {}
                ".." => {
                    let parent = self.node(current)?.parent;
                    if let Some(p) = parent {
                        current = p;
                    }
                }
                name => {
                    current = self
                        .lookup_child_by_name(current, name)
                        .ok_or("path not found")?;
                }
            }
        }

        Ok(current)
    }

    fn resolve(&self, path: &str) -> Result<u64, &'static str> {
        self.resolve_from(self.cwd, path)
    }

    fn resolve_parent_and_name(&self, path: &str) -> Result<(u64, String), &'static str> {
        let mut parts: Vec<&str> = Self::path_parts(path).collect();
        let name = parts.pop().ok_or("missing name")?;
        if name == "." || name == ".." {
            return Err("invalid name");
        }

        let parent_path = if path.starts_with('/') {
            if parts.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", parts.join("/"))
            }
        } else {
            parts.join("/")
        };

        let parent = if parent_path.is_empty() {
            self.cwd
        } else {
            self.resolve(&parent_path)?
        };

        Ok((parent, name.to_string()))
    }

    fn insert_node(
        &mut self,
        parent_inode: u64,
        name: String,
        kind: FileType,
    ) -> Result<u64, &'static str> {
        let parent = self.node(parent_inode)?;
        if parent.kind != FileType::Directory {
            return Err("parent is not a directory");
        }
        if self.lookup_child_by_name(parent_inode, &name).is_some() {
            return Err("already exists");
        }

        let inode = self.next_inode();
        let node = Node {
            inode,
            name,
            kind,
            parent: Some(parent_inode),
            children: Vec::new(),
            data: Vec::new(),
        };

        self.nodes.push(Some(node));
        self.node_mut(parent_inode)?.children.push(inode);
        Ok(inode)
    }

    fn remove_inode(&mut self, inode: u64) -> Result<(), &'static str> {
        if inode == self.root {
            return Err("cannot remove root");
        }

        let node = self.node(inode)?.clone();
        if node.kind == FileType::Directory && !node.children.is_empty() {
            return Err("directory not empty");
        }

        if let Some(parent_inode) = node.parent {
            let parent = self.node_mut(parent_inode)?;
            parent.children.retain(|&c| c != inode);
        }

        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        if let Some(slot) = self.nodes.get_mut(idx) {
            *slot = None;
        }

        if self.cwd == inode {
            self.cwd = self.root;
        }

        Ok(())
    }

    fn cwd_path(&self) -> String {
        self.path_for_inode(self.cwd)
    }

    fn path_for_inode(&self, inode: u64) -> String {
        if inode == self.root {
            return "/".to_string();
        }

        let mut parts: Vec<String> = Vec::new();
        let mut current = inode;

        loop {
            let node = match self.node(current) {
                Ok(n) => n,
                Err(_) => break,
            };

            if current == self.root {
                break;
            }

            parts.push(node.name.clone());
            current = node.parent.unwrap_or(self.root);
            if current == self.root {
                break;
            }
        }

        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    fn normalized_path(&self, path: &str) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !path.starts_with('/') {
            let cwd = self.cwd_path();
            for p in cwd.split('/').filter(|p| !p.is_empty()) {
                parts.push(p.to_string());
            }
        }

        for p in Self::path_parts(path) {
            match p {
                "." => {}
                ".." => {
                    let _ = parts.pop();
                }
                name => parts.push(name.to_string()),
            }
        }

        if parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", parts.join("/"))
        }
    }
}

impl FileSystem for TmpFs {
    fn create(&mut self, path: &str) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent_and_name(path)?;
        self.insert_node(parent, name, FileType::File)?;
        Ok(())
    }

    fn open(&self, path: &str) -> Result<VNode, &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node(inode)?;
        Ok(VNode {
            inode: node.inode,
            name: node.name.clone(),
            kind: node.kind,
        })
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        Ok(node.data.clone())
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node_mut(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        node.data.clear();
        node.data.extend_from_slice(data);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), &'static str> {
        let inode = self.resolve(path)?;
        self.remove_inode(inode)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent_and_name(path)?;
        self.insert_node(parent, name, FileType::Directory)?;
        Ok(())
    }

    fn readdir(&self, path: &str) -> Result<Vec<String>, &'static str> {
        let inode = if path.is_empty() { self.cwd } else { self.resolve(path)? };
        let node = self.node(inode)?;
        if node.kind != FileType::Directory {
            return Err("not a directory");
        }

        let mut out = Vec::new();
        for &child_inode in &node.children {
            let child = self.node(child_inode)?;
            out.push(child.name.clone());
        }
        out.sort();
        Ok(out)
    }
}

struct VfsState {
    fs: TmpFs,
}

impl VfsState {
    fn new() -> Self {
        let mut fs = TmpFs::new();
        seed_standard_tree(&mut fs);
        Self { fs }
    }
}

fn seed_standard_tree(fs: &mut TmpFs) {
    let roots = [
        "/system",
        "/dev",
        "/proc",
        "/sys",
        "/home",
        "/tmp",
        "/var",
        "/boot",
        "/bin",
        "/usr",
        "/etc",
    ];
    for path in roots {
        let _ = fs.mkdir(path);
    }

    let sys_nodes = [
        "/sys/devices",
        "/sys/drivers",
        "/sys/memory",
        "/sys/scheduler",
        "/sys/storage",
        "/sys/network",
        "/sys/services",
    ];
    for path in sys_nodes {
        let _ = fs.mkdir(path);
    }

    let user_programs = ["hello", "true", "false", "argc", "env", "fail"];
    for name in user_programs {
        let _ = fs.create(format!("/bin/{}", name).as_str());
    }
}

fn is_sys_path(path: &str) -> bool {
    path == "/sys" || path.starts_with("/sys/")
}

static VFS: StaticCell<Option<VfsState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_vfs<R>(f: impl FnOnce(&mut VfsState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *VFS.get();
            if slot.is_none() {
                *slot = Some(VfsState::new());
            }
            slot.as_mut().expect("vfs not initialized")
        };
        f(state)
    };
    unlock();
    out
}

pub fn init() {
    with_vfs(|_| {});
}

pub fn mkdir(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        vfs.fs.mkdir(path)?;
        object_manager::log_event(&format!("Directory created: {}", abs));
        Ok(())
    })
}

pub fn touch(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        vfs.fs.create(path)?;
        object_manager::log_event(&format!("File created: {}", abs));
        Ok(())
    })
}

pub fn ls(path: Option<&str>) -> Result<Vec<String>, &'static str> {
    with_vfs(|vfs| {
        let req = path.unwrap_or(".");
        let abs = vfs.fs.normalized_path(req);
        if is_sys_path(&abs) {
            return object_manager::sys_readdir(&abs).ok_or("not a directory");
        }

        vfs.fs.readdir(req)
    })
}

pub fn cat(path: &str) -> Result<String, &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            let lines = object_manager::sys_read(&abs).ok_or("not a file")?;
            return Ok(lines.join("\n"));
        }

        let bytes = vfs.fs.read(path)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    })
}

pub fn rm(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        vfs.fs.remove(path)?;
        object_manager::log_event(&format!("Object removed: {}", abs));
        Ok(())
    })
}

pub fn cd(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let inode = vfs.fs.resolve(path)?;
        let node = vfs.fs.node(inode)?;
        if node.kind != FileType::Directory {
            return Err("not a directory");
        }
        vfs.fs.cwd = inode;
        Ok(())
    })
}

pub fn pwd() -> String {
    with_vfs(|vfs| vfs.fs.cwd_path())
}

pub fn write(path: &str, data: &[u8]) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }
        vfs.fs.write(path, data)
    })
}

pub fn open(path: &str) -> Result<VNode, &'static str> {
    with_vfs(|vfs| vfs.fs.open(path))
}
