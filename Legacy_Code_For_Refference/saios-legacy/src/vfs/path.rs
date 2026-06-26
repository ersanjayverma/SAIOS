//! Path resolution — walks the mount table and inode tree.

use super::{FileType, Inode, VfsError, VfsResult};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

const MAX_SYMLINK_DEPTH: u32 = 40;

/// Resolve an absolute path to an inode.
pub fn resolve(path: &str, depth: u32) -> VfsResult<Arc<Inode>> {
    if depth > MAX_SYMLINK_DEPTH {
        return Err(VfsError::Loop);
    }

    let path = normalise(&super::namespace::translate_path(path));
    if path == "/" {
        return super::get_mount_root("/").ok_or(VfsError::NotFound);
    }

    // Find the deepest mount point that is a prefix of `path`
    let (mount_root, rel) = find_mount(&path)?;

    // Walk the relative path from the mount root
    let mut current = mount_root;
    for component in rel.split('/').filter(|s| !s.is_empty()) {
        current = current.ops.lookup(component)?;

        // Follow symlinks
        if current.ftype == FileType::SymLink {
            let target = current.ops.readlink()?;
            let resolved = if target.starts_with('/') {
                target
            } else {
                // relative symlink — resolve relative to parent
                let parent_path = parent_of_path(&path);
                format!("{}/{}", parent_path, target)
            };
            current = resolve(&resolved, depth + 1)?;
        }
    }
    Ok(current)
}

/// Resolve the parent directory of a path, returning (parent_inode, filename).
pub fn resolve_parent(path: &str) -> VfsResult<(Arc<Inode>, String)> {
    let path = normalise(&super::namespace::translate_path(path));
    let (parent_str, name) = split_path(&path);
    let parent = resolve(&parent_str, 0)?;
    Ok((parent, name.to_string()))
}

// -- Helpers ----------------------------------------------------------------

fn find_mount(path: &str) -> VfsResult<(Arc<Inode>, String)> {
    super::active_mount_namespace().find_mount(path)
}

pub fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return String::from("/");
    }
    format!("/{}", parts.join("/"))
}

fn split_path(path: &str) -> (String, &str) {
    match path.rfind('/') {
        Some(0) | None => (String::from("/"), path.trim_start_matches('/')),
        Some(i) => (path[..i].to_string(), &path[i + 1..]),
    }
}

fn parent_of_path(path: &str) -> String {
    let (p, _) = split_path(path);
    p
}
