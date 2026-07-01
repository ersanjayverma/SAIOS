use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::object_manager::{self, Health, ObjectId, ObjectMetadata, ObjectStatus, ObjectType, PropertyMap};
use crate::vfs;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SaifsNodeKind {
    File,
    Directory,
    Object,
    Virtual,
}

pub struct SaifsHandle {
    path: String,
    object_id: Option<ObjectId>,
    kind: SaifsNodeKind,
}

impl SaifsHandle {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn object_id(&self) -> Option<ObjectId> {
        self.object_id
    }

    pub fn kind(&self) -> SaifsNodeKind {
        self.kind
    }

    pub fn properties(&self) -> Result<PropertyMap, &'static str> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.properties);
        }

        match self.kind {
            SaifsNodeKind::File => {
                let data = vfs::cat(&self.path)?;
                let mut props = Vec::new();
                props.push(object_manager::Property {
                    key: "type".to_string(),
                    value: "file".to_string(),
                });
                props.push(object_manager::Property {
                    key: "size".to_string(),
                    value: data.len().to_string(),
                });
                Ok(props)
            }
            SaifsNodeKind::Directory => {
                let children = vfs::ls(Some(&self.path))?;
                let mut props = Vec::new();
                props.push(object_manager::Property {
                    key: "type".to_string(),
                    value: "directory".to_string(),
                });
                props.push(object_manager::Property {
                    key: "children".to_string(),
                    value: children.len().to_string(),
                });
                Ok(props)
            }
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Ok(Vec::new()),
        }
    }

    pub fn health(&self) -> Result<Health, &'static str> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.health);
        }

        match self.kind {
            SaifsNodeKind::File | SaifsNodeKind::Directory => Ok(Health::Healthy),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err("health unavailable"),
        }
    }

    pub fn children(&self) -> Result<Vec<String>, &'static str> {
        match self.kind {
            SaifsNodeKind::File => Err("not a directory"),
            SaifsNodeKind::Directory | SaifsNodeKind::Virtual => vfs::ls(Some(&self.path)),
            SaifsNodeKind::Object => {
                if let Some(meta) = object_manager::metadata(&self.path) {
                    let mut out = Vec::new();
                    for child_id in meta.children {
                        if let Some((_, name, _)) = object_manager::lookup_by_id(child_id) {
                            out.push(name);
                        }
                    }
                    out.sort();
                    Ok(out)
                } else {
                    vfs::ls(Some(&self.path))
                }
            }
        }
    }

    pub fn metadata(&self) -> Option<ObjectMetadata> {
        object_manager::metadata(&self.path)
    }

    pub fn status(&self) -> Result<ObjectStatus, &'static str> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.status);
        }
        match self.kind {
            SaifsNodeKind::File | SaifsNodeKind::Directory => Ok(ObjectStatus::Online),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err("status unavailable"),
        }
    }

    pub fn object_type(&self) -> Result<ObjectType, &'static str> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.kind);
        }

        match self.kind {
            SaifsNodeKind::File => Ok(ObjectType::File),
            SaifsNodeKind::Directory => Ok(ObjectType::Volume),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err("object type unavailable"),
        }
    }
}

pub fn init() {
    object_manager::init();
    vfs::init();
}

pub fn open(path: &str) -> Result<SaifsHandle, &'static str> {
    init();

    let path = if path.is_empty() { "/" } else { path };

    if path == "/sys" || path.starts_with("/sys/") {
        if object_manager::metadata(path).is_some() {
            let object_id = object_manager::metadata(path).map(|m| m.id);
            return Ok(SaifsHandle {
                path: path.to_string(),
                object_id,
                kind: SaifsNodeKind::Object,
            });
        }

        if object_manager::sys_readdir(path).is_some() {
            return Ok(SaifsHandle {
                path: path.to_string(),
                object_id: None,
                kind: SaifsNodeKind::Virtual,
            });
        }

        if object_manager::sys_read(path).is_some() {
            return Ok(SaifsHandle {
                path: path.to_string(),
                object_id: None,
                kind: SaifsNodeKind::Virtual,
            });
        }
    }

    let node = vfs::open(path)?;
    let kind = match node.kind {
        vfs::FileType::File => SaifsNodeKind::File,
        vfs::FileType::Directory => SaifsNodeKind::Directory,
    };

    let object_id = object_manager::metadata(path).map(|m| m.id);
    Ok(SaifsHandle {
        path: path.to_string(),
        object_id,
        kind,
    })
}

pub fn read_text(path: &str) -> Result<String, &'static str> {
    init();
    vfs::cat(path)
}

pub fn list(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    vfs::ls(Some(path))
}

pub fn mkdir(path: &str) -> Result<(), &'static str> {
    init();
    vfs::mkdir(path)
}

pub fn touch(path: &str) -> Result<(), &'static str> {
    init();
    vfs::touch(path)
}

pub fn remove(path: &str) -> Result<(), &'static str> {
    init();
    vfs::rm(path)
}

pub fn explain(path: &str) -> Result<Vec<String>, &'static str> {
    init();

    if path == "/sys" || path.starts_with("/sys/") {
        return object_manager::explain(path);
    }

    Err("explain currently supported for object paths")
}

pub fn diagnose(path: &str) -> Result<Vec<String>, &'static str> {
    init();

    if path == "/sys" || path.starts_with("/sys/") {
        return object_manager::diagnose(path);
    }

    Err("diagnose currently supported for object paths")
}
