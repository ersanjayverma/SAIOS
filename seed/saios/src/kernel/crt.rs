use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CrtAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct LibcSurface {
    pub crt0: bool,
    pub argv_envp: bool,
    pub malloc_free: bool,
    pub printf: bool,
}

#[derive(Clone, Debug)]
pub struct CrtStartupBlock {
    pub program: String,
    pub argc: usize,
    pub argv: Vec<String>,
    pub envp: Vec<(String, String)>,
}

const ABI_VERSION: CrtAbiVersion = CrtAbiVersion {
    major: 1,
    minor: 0,
    patch: 0,
};

const LIBC_SURFACE: LibcSurface = LibcSurface {
    crt0: true,
    argv_envp: true,
    malloc_free: true,
    printf: true,
};

pub fn abi_version() -> CrtAbiVersion {
    ABI_VERSION
}

pub fn libc_surface() -> LibcSurface {
    LIBC_SURFACE
}

pub fn prepare_startup_block(
    program: &str,
    args: &[&str],
    env: &[(String, String)],
) -> CrtStartupBlock {
    let mut argv = Vec::new();
    argv.push(program.to_string());
    for a in args {
        argv.push((*a).to_string());
    }

    CrtStartupBlock {
        program: program.to_string(),
        argc: argv.len(),
        argv,
        envp: env.to_vec(),
    }
}
