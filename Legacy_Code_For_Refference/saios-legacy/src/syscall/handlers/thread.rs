use super::{EINVAL, ENOSYS, write_user};
use crate::process;

pub fn sys_clone(flags: u64, stack: u64, parent_tid: u64, child_tid: u64, tls: u64) -> i64 {
    use crate::process::thread::CLONE_THREAD;
    if flags & CLONE_THREAD != 0 {
        let (rip, rflags) =
            process::with_current_process(|p| (p.rip, p.rflags)).unwrap_or((0, 0x202));
        return crate::process::thread::do_clone(
            flags, stack, parent_tid, child_tid, tls, rip, rflags,
        );
    }
    super::proc_handlers::sys_fork()
}

pub fn sys_arch_prctl(code: u64, addr: u64) -> i64 {
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;
    const ARCH_SET_GS: u64 = 0x1001;
    const ARCH_GET_GS: u64 = 0x1004;

    match code {
        ARCH_SET_FS => {
            if crate::process::with_current_process_mut(|p| p.fs_base.fs_base = addr).is_some() {
                unsafe {
                    crate::arch::process::set_fs_base(addr);
                }
                0
            } else {
                EINVAL
            }
        }
        ARCH_GET_FS => crate::process::with_current_process(|p| {
            unsafe {
                write_user(addr, p.fs_base.fs_base);
            }
            0
        })
        .unwrap_or(EINVAL),
        ARCH_SET_GS => {
            if crate::process::with_current_process_mut(|p| p.fs_base.gs_base = addr).is_some() {
                unsafe {
                    if crate::arch::syscall::kernel_gs_active() {
                        crate::arch::process::set_kernel_gs_base(addr);
                    } else {
                        crate::arch::process::set_gs_base(addr);
                    }
                }
                0
            } else {
                EINVAL
            }
        }
        ARCH_GET_GS => crate::process::with_current_process(|p| {
            unsafe {
                write_user(addr, p.fs_base.gs_base);
            }
            0
        })
        .unwrap_or(EINVAL),
        _ => EINVAL,
    }
}

pub fn sys_set_tid_address(_tidptr: u64) -> i64 {
    super::proc_handlers::sys_getpid()
}

pub fn sys_setfsuid(_uid: u64) -> i64 {
    ENOSYS
}

pub fn sys_setfsgid(_gid: u64) -> i64 {
    ENOSYS
}
