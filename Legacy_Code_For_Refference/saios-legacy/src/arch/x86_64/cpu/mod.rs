//! x86_64 CPU-specific subsystem implementations.

pub mod gdt;
pub mod tables;

pub use gdt::*;
pub use tables::*;
