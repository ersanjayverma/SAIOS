//! SAIOS libc compatibility layer
//!
//! This module provides user-space libc wrappers that use kernel primitives
//! for threading, synchronization, and signal handling.

pub mod pthread;
