//! Symbol resolution — walks all loaded shared objects to find a symbol.

use alloc::string::String;

pub fn lookup(name: &str) -> Option<u64> {
    let loaded = super::LOADED.lock();
    for so in loaded.values() {
        if let Some(&addr) = so.syms.get(name) {
            return Some(addr);
        }
    }
    None
}
