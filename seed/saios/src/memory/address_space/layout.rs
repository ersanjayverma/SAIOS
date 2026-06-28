use crate::memory::constants::{KERNEL_SPACE_START, USER_SPACE_END};
use crate::memory::types::VirtAddr;

pub fn is_kernel_address(address: VirtAddr) -> bool {
    address.as_u64() >= KERNEL_SPACE_START
}

pub fn is_user_address(address: VirtAddr) -> bool {
    address.as_u64() <= USER_SPACE_END
}
