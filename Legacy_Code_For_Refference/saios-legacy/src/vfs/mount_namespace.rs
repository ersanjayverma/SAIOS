use super::{Inode, VfsError, VfsResult, path};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone)]
pub struct MountPoint {
    pub path: String,
    pub root: Arc<Inode>,
    pub fs_type: String,
}

pub struct MountNamespace {
    mounts: Mutex<Vec<MountPoint>>,
}

impl Default for MountNamespace {
    fn default() -> Self {
        Self::new()
    }
}

impl MountNamespace {
    pub const fn new() -> Self {
        Self {
            mounts: Mutex::new(Vec::new()),
        }
    }

    pub fn insert_mount(&self, path: &str, fs_type: &str, root: Arc<Inode>) {
        let mut mounts = self.mounts.lock();
        let path = path::normalise(path);
        mounts.retain(|m| m.path != path);
        mounts.push(MountPoint {
            path,
            root,
            fs_type: String::from(fs_type),
        });
    }

    pub fn remove_mount(&self, path: &str) -> VfsResult<()> {
        let mut mounts = self.mounts.lock();
        let path = path::normalise(path);
        let before = mounts.len();
        mounts.retain(|m| m.path != path);
        if mounts.len() == before {
            return Err(VfsError::NotFound);
        }
        Ok(())
    }

    pub fn get_mount_root(&self, path: &str) -> Option<Arc<Inode>> {
        self.lookup_mount(path).map(|m| m.root)
    }

    pub fn lookup_mount(&self, path: &str) -> Option<MountPoint> {
        let path = path::normalise(path);
        let mounts = self.mounts.lock();
        let mut best: Option<&MountPoint> = None;
        for m in mounts.iter() {
            if path.starts_with(m.path.as_str())
                && (best.is_none() || m.path.len() > best.expect("best mount").path.len())
            {
                best = Some(m);
            }
        }
        best.cloned()
    }

    pub fn find_mount(&self, path: &str) -> VfsResult<(Arc<Inode>, String)> {
        let path = path::normalise(path);
        let mounts = self.mounts.lock();
        let mut best_len = 0usize;
        let mut best_root: Option<Arc<Inode>> = None;
        let mut best_rel = path.as_str();

        for m in mounts.iter() {
            let mp = m.path.trim_end_matches('/');
            if path == mp || path.starts_with(&alloc::format!("{}/", mp)) || mp == "/" {
                let len = m.path.len();
                if len >= best_len {
                    best_len = len;
                    best_root = Some(m.root.clone());
                    best_rel = if mp == "/" {
                        path.trim_start_matches('/')
                    } else {
                        path[mp.len()..].trim_start_matches('/')
                    };
                }
            }
        }
        match best_root {
            Some(root) => Ok((root, String::from(best_rel))),
            None => Err(VfsError::NotFound),
        }
    }

    pub fn list_mounts(&self) -> Vec<(String, String)> {
        self.mounts
            .lock()
            .iter()
            .map(|m| (m.path.clone(), m.fs_type.clone()))
            .collect()
    }

    pub fn clone_namespace(&self) -> Arc<Self> {
        Arc::new(Self {
            mounts: Mutex::new(self.mounts.lock().clone()),
        })
    }
}
