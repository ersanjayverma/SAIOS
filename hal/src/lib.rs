//! Hardware abstraction layer (HAL) for SAIOS.
//!
//! Exposes architecture-specific code under [`arch`]. Currently only x86_64 is
//! implemented.

#![no_std]
pub mod arch;
