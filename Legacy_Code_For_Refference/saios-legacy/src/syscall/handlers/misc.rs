use super::{EBADF, EFAULT, EINTR, ENOSYS, ENOTTY, write_user};

pub fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    const TCGETS: u64 = 0x5401;
    const TCSETS: u64 = 0x5402;
    const TCSETSW: u64 = 0x5403;
    const TIOCGWINSZ: u64 = 0x5413;
    const TIOCSWINSZ: u64 = 0x5414;
    const TIOCGPGRP: u64 = 0x540F;
    const TIOCSPGRP: u64 = 0x5410;
    const TIOCSCTTY: u64 = 0x5480;
    const TIOCGSID: u64 = 0x5429;
    const FIONREAD: u64 = 0x541B;
    const FIOCLEX: u64 = 0x5451;
    const FIONBIO: u64 = 0x5421;

    match request {
        TCGETS | TCSETS | TCSETSW => 0,
        TIOCGWINSZ => {
            if arg != 0 {
                unsafe {
                    core::ptr::write_volatile(arg as *mut u16, 25);
                    core::ptr::write_volatile((arg + 2) as *mut u16, 80);
                    core::ptr::write_volatile((arg + 4) as *mut u16, 640);
                    core::ptr::write_volatile((arg + 6) as *mut u16, 400);
                }
            }
            0
        }
        TIOCGPGRP => {
            unsafe { write_user(arg, crate::tty::io::get_fg_pgid()) };
            0
        }
        TIOCSPGRP => {
            if arg == 0 {
                return EFAULT;
            }
            let pgid = unsafe { core::ptr::read_volatile(arg as *const u32) };
            if crate::tty::io::set_fg_pgid_if_process_group_exists(pgid) {
                0
            } else {
                -3
            }
        }
        TIOCSCTTY => {
            crate::tty::io::set_controlling_tty(crate::tty::DEV_CONSOLE);
            0
        }
        TIOCGSID => {
            unsafe { write_user(arg, crate::tty::io::get_session_id()) };
            0
        }
        TIOCSWINSZ => 0,
        FIONREAD => {
            unsafe { write_user(arg, 0u32) };
            0
        }
        FIOCLEX => 0,
        FIONBIO => 0,
        _ => {
            crate::serial_println!("[ioctl] unhandled fd={} req={:#x}", fd, request);
            ENOTTY
        }
    }
}

pub fn sys_gettimeofday(tv_ptr: u64, _tz_ptr: u64) -> i64 {
    if tv_ptr != 0 {
        let (secs, nsecs) = crate::time::realtime();
        unsafe {
            core::ptr::write_volatile(tv_ptr as *mut u64, secs);
            core::ptr::write_volatile((tv_ptr + 8) as *mut u64, nsecs / 1000);
        }
    }
    0
}

pub fn sys_time(t_ptr: u64) -> i64 {
    let (secs, _) = crate::time::realtime();
    if t_ptr != 0 {
        unsafe {
            write_user(t_ptr, secs);
        }
    }
    secs as i64
}

pub fn sys_internal_shell() -> i64 {
    crate::serial_println!("[userspace-shell] entering internal shell");
    crate::process::USER_MODE_ACTIVE.store(false, core::sync::atomic::Ordering::Relaxed);
    if crate::arch::syscall::kernel_gs_active() {
        unsafe {
            crate::arch::process::swapgs();
        }
        crate::arch::syscall::mark_kernel_gs_active(false);
    }
    let mut shell = crate::shell::Shell::new();
    shell.run();
}

pub fn sys_clock_gettime(clock_id: u64, tp_ptr: u64) -> i64 {
    let (secs, nsecs) = match clock_id {
        1 | 4 | 7 => {
            let up = crate::time::uptime_ns();
            (up / 1_000_000_000, up % 1_000_000_000)
        }
        _ => crate::time::realtime(),
    };
    if tp_ptr != 0 {
        unsafe {
            write_user(tp_ptr, secs);
            write_user(tp_ptr + 8, nsecs);
        }
    }
    0
}

pub fn sys_clock_getres(_clock_id: u64, tp_ptr: u64) -> i64 {
    if tp_ptr != 0 {
        unsafe {
            write_user(tp_ptr, 0u64);
            write_user(tp_ptr + 8, 1_000_000u64);
        }
    }
    0
}

pub fn sys_nanosleep(req_ptr: u64, _rem_ptr: u64) -> i64 {
    if req_ptr == 0 {
        return EFAULT;
    }
    let secs = unsafe { core::ptr::read_volatile(req_ptr as *const u64) };
    let nsecs = unsafe { core::ptr::read_volatile((req_ptr + 8) as *const u64) };
    let ticks = secs * 100 + nsecs / 10_000_000;
    if ticks == 0 {
        return 0;
    }
    let target = crate::shell::commands::boot_ticks() + ticks;
    while crate::shell::commands::boot_ticks() < target {
        let interrupted =
            crate::process::with_current_process(|proc| proc.signals.is_pending()).unwrap_or(false);
        if interrupted {
            return EINTR;
        }
        crate::interrupts::block_until_tick(target);
    }
    0
}

pub fn sys_alarm(_secs: u64) -> i64 {
    ENOSYS
}

pub fn sys_setitimer(_which: u64, _new: u64, _old: u64) -> i64 {
    ENOSYS
}

pub fn sys_getrlimit(_res: u64, rlim: u64) -> i64 {
    if rlim != 0 {
        unsafe {
            core::ptr::write_volatile(rlim as *mut u64, u64::MAX);
            core::ptr::write_volatile((rlim + 8) as *mut u64, u64::MAX);
        }
    }
    0
}

pub fn sys_getrusage(_who: u64, usage: u64) -> i64 {
    if usage != 0 {
        unsafe {
            core::ptr::write_bytes(usage as *mut u8, 0, 144);
        }
    }
    0
}

pub fn sys_sysinfo(info: u64) -> i64 {
    if info == 0 {
        return EFAULT;
    }
    let (total, free, _) = crate::memory::frame_stats();
    unsafe {
        core::ptr::write_volatile(info as *mut u64, 0u64);
        core::ptr::write_volatile((info + 8) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 16) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 24) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 32) as *mut u64, (total * 4096) as u64);
        core::ptr::write_volatile((info + 40) as *mut u64, (free * 4096) as u64);
        core::ptr::write_volatile((info + 48) as *mut u64, (free * 4096) as u64);
        core::ptr::write_volatile((info + 56) as *mut u64, (total * 4096) as u64);
        core::ptr::write_volatile((info + 64) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 72) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 80) as *mut u64, 0u64);
        core::ptr::write_volatile((info + 88) as *mut u64, 1u16 as u64);
        core::ptr::write_volatile((info + 96) as *mut u64, 4096u64);
    }
    0
}

pub fn sys_times(_buf: u64) -> i64 {
    ENOSYS
}

pub fn sys_futex(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i64 {
    crate::ipc::futex::sys_futex(a, b as u32, c as u32, d, e, f as u32)
}

pub fn sys_getrandom(buf: u64, len: u64, _flags: u64) -> i64 {
    if buf == 0 {
        return EFAULT;
    }
    let data = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len as usize) };
    let mut s = 0xDEAD_BEEF_1234_5678u64;
    for b in data.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    len as i64
}

pub fn sys_clock_nanosleep(_clock: u64, _flags: u64, req: u64, rem: u64) -> i64 {
    sys_nanosleep(req, rem)
}

pub fn sys_stub(num: u64) -> i64 {
    crate::serial_println!("[syscall] #{} not implemented", num);
    ENOSYS
}

pub fn sys_saios_write(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    const MAX_PUTS: usize = 4096;
    let (fd, user_ptr, len) = if fd >= crate::process::USER_TEXT_BASE {
        (1, fd, buf_ptr)
    } else {
        (fd, buf_ptr, len)
    };
    if fd != 1 && fd != 2 {
        return EBADF;
    }
    let len = (len as usize).min(MAX_PUTS);
    crate::syscall::trace_write_enter(fd, len as u64, user_ptr);
    if user_ptr == 0
        || user_ptr < crate::process::USER_TEXT_BASE
        || user_ptr
            .checked_add(len as u64)
            .is_none_or(|end| end > crate::process::USER_TOP + 1)
    {
        crate::syscall::trace_write_exit(EFAULT);
        return EFAULT;
    }
    let mut written = 0usize;
    for i in 0..len {
        let byte = match read_user_byte(user_ptr + i as u64) {
            Some(b) => b,
            None => {
                crate::syscall::trace_write_exit(EFAULT);
                return EFAULT;
            }
        };
        if byte == 0 {
            break;
        }
        write_serial_byte(byte);
        written += 1;
    }
    crate::syscall::trace_write_exit(written as i64);
    written as i64
}

pub fn sys_saios_puts(user_ptr: u64, max_len: u64) -> i64 {
    sys_saios_write(1, user_ptr, max_len)
}

pub fn sys_saios_putc(fd: u64, byte: u64) -> i64 {
    let (fd, byte) = if fd != 1 && fd != 2 && fd <= 0xFF {
        (1, fd)
    } else {
        (fd, byte)
    };
    if fd != 1 && fd != 2 {
        return EBADF;
    }
    write_serial_byte((byte & 0xFF) as u8);
    1
}

fn read_user_byte(virt: u64) -> Option<u8> {
    use crate::memory::paging;
    let pml4 = paging::active_pml4();
    let (phys, flags) = paging::translate_entry_in(pml4, virt)?;
    if flags & paging::PTE_PRESENT == 0 {
        return None;
    }
    Some(unsafe { *(phys as *const u8) })
}

fn write_serial_byte(byte: u8) {
    use crate::driver::serial::SERIAL;
    crate::arch::without_interrupts(|| {
        let mut serial = SERIAL.lock();
        if byte == b'\n' {
            serial.write_byte(b'\r');
        }
        serial.write_byte(byte);
    });
}
