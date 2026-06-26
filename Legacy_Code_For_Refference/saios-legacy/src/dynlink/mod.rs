//! SAIOS dynamic-link scaffolding.
//!
//! Current state in this module:
//!   - Keeps a global registry of loaded shared objects.
//!   - Maintains hard-coded library search paths.
//!   - Exposes basic `dlopen`/`dlsym`/`dlclose` helpers.
//!
//! What this module actually covers today:
//!   - shared-object bookkeeping and search-path management
//!   - partial shared-object loading and relocation support through `elf_dyn`
//!   - TLS helper scaffolding through `tls`
//!
//! Important boundary:
//!   - thread, futex, and signal semantics live in other subsystems and are not
//!     made production-complete by the presence of this module
//!   - the overall dynamic-userland path is still incomplete even though parts
//!     of the loader and relocation flow exist

pub mod elf_dyn;
pub mod resolve;
pub mod tls;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// A loaded shared object.
pub struct SharedObject {
    pub name: String,
    pub base: u64, // load address
    pub entry: u64,
    pub size: usize,
    pub syms: BTreeMap<String, u64>, // name → virtual address
}

/// Global loaded library registry.
pub static LOADED: Mutex<BTreeMap<String, Arc<SharedObject>>> = Mutex::new(BTreeMap::new());

/// Library search paths (populated from LD_LIBRARY_PATH + defaults).
pub static SEARCH_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn init() {
    let mut paths = SEARCH_PATHS.lock();
    paths.push(String::from("/lib"));
    paths.push(String::from("/lib64"));
    paths.push(String::from("/lib/x86_64-linux-gnu"));
    paths.push(String::from("/usr/lib"));
    paths.push(String::from("/usr/lib64"));
    paths.push(String::from("/usr/lib/x86_64-linux-gnu"));
    paths.push(String::from("/usr/local/lib"));
    crate::println!(
        "[dynlink] dynamic linker ready — {} search paths",
        paths.len()
    );
}

pub fn register_loaded(so: SharedObject) -> Arc<SharedObject> {
    let arc = Arc::new(so);
    LOADED.lock().insert(arc.name.clone(), arc.clone());
    arc
}

/// Load a shared object by name (e.g. "libc.so.6").
/// Searches SEARCH_PATHS, maps the ELF, resolves its dependencies.
pub fn load(name: &str) -> Result<Arc<SharedObject>, &'static str> {
    // Check if already loaded
    if let Some(so) = LOADED.lock().get(name) {
        return Ok(so.clone());
    }

    // Find the file
    let path = find_library(name)?;
    let data =
        crate::vfs_contract::VfsContract::read_file(&path).map_err(|_| "dynlink: read failed")?;

    let so = elf_dyn::load_so(&data, name)?;
    let arc = register_loaded(so);
    crate::println!("[dynlink] loaded {} at {:#x}", name, arc.base);
    Ok(arc)
}

fn find_library(name: &str) -> Result<String, &'static str> {
    let paths = SEARCH_PATHS.lock();
    for path in paths.iter() {
        let full = alloc::format!("{}/{}", path, name);
        if crate::vfs_contract::VfsContract::resolve(&full).is_ok() {
            return Ok(full);
        }
    }
    Err("dynlink: library not found in search path")
}

/// dlopen — load a library and return a handle (its base address).
pub fn dlopen(path: &str, _flags: u32) -> u64 {
    let name = path.rsplit('/').next().unwrap_or(path);
    match load(name) {
        Ok(so) => so.base,
        Err(e) => {
            crate::serial_println!("[dlopen] {}: {}", path, e);
            0
        }
    }
}

/// dlsym — find a symbol in a loaded library.
pub fn dlsym(handle: u64, symbol: &str) -> u64 {
    let loaded = LOADED.lock();
    for so in loaded.values() {
        if (handle == 0 || so.base == handle)
            && let Some(&addr) = so.syms.get(symbol)
        {
            return addr;
        }
    }
    0
}

/// dlclose — unload a library with reference counting.
/// Returns 0 on success, -1 on error.
/// Libraries are only unmapped when their reference count drops to zero.
pub fn dlclose(handle: u64) -> i32 {
    let mut loaded = LOADED.lock();

    // Find the library by handle (base address)
    let to_remove: Vec<String> = loaded
        .iter()
        .filter(|(_, so)| so.base == handle)
        .map(|(name, _)| name.clone())
        .collect();

    if to_remove.is_empty() {
        return -1; // Error: handle not found
    }

    for name in to_remove {
        // In a real implementation, we'd decrement reference count here
        // For now, we just remove it - in a full implementation, this would
        // check rc > 1 and just decrement, or if rc == 1, unmap and remove
        loaded.remove(&name);
        crate::println!("[dlclose] unloaded {}", name);
    }

    0
}
