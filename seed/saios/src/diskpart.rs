//! DISKPART compatibility shim.
//!
//! The canonical implementation for disk/volume management lives in the
//! interactive `volumes` shell command. This module keeps `exec diskpart ...`
//! working by forwarding all arguments to that command path.

use alloc::string::String;

pub fn run(args: &[&str], env: &[(String, String)]) -> Result<i32, &'static str> {
    crate::shell::run_diskpart_alias(args, env)
}
