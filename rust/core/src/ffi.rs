//! The handful of libc calls the standard library doesn't wrap. Everything here is
//! a thin, safe wrapper; the `unsafe` stays in this file.

use std::os::raw::{c_char, c_int, c_long};

#[repr(C)]
struct SigSet {
    bits: [u64; 16], // glibc's sigset_t is 1024 bits
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

extern "C" {
    fn sigemptyset(set: *mut SigSet) -> c_int;
    fn sigaddset(set: *mut SigSet, sig: c_int) -> c_int;
    fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn sigwait(set: *const SigSet, sig: *mut c_int) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn getppid() -> c_int;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
    fn flock(fd: c_int, op: c_int) -> c_int;
    fn time(out: *mut i64) -> i64;
    fn tzset();
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    fn mktime(tm: *mut Tm) -> i64;
    fn __libc_current_sigrtmin() -> c_int;
}

pub const SIGUSR1: i32 = 10;
pub const SIGUSR2: i32 = 12;
pub const SIGTERM: i32 = 15;
const SIG_BLOCK: c_int = 0;
const POLLERR: i16 = 0x8;
const POLLHUP: i16 = 0x10;
const LOCK_EX: c_int = 2;
const LOCK_NB: c_int = 4;

/// Block `signals` in this thread and every thread it starts afterwards, so they
/// can be collected with [`wait_signal`] instead of interrupting anything.
/// Call it before spawning threads.
pub fn block_signals(signals: &[i32]) {
    // SAFETY: the set is initialised by sigemptyset before use.
    unsafe {
        let mut set: SigSet = std::mem::zeroed();
        sigemptyset(&mut set);
        for &s in signals {
            sigaddset(&mut set, s);
        }
        pthread_sigmask(SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Wait for one of `signals` (blocked earlier with [`block_signals`]).
pub fn wait_signal(signals: &[i32]) -> i32 {
    // SAFETY: as above; sigwait writes the signal number into `got`.
    unsafe {
        let mut set: SigSet = std::mem::zeroed();
        sigemptyset(&mut set);
        for &s in signals {
            sigaddset(&mut set, s);
        }
        let mut got: c_int = 0;
        if sigwait(&set, &mut got) == 0 {
            got
        } else {
            0
        }
    }
}

pub fn send_signal(pid: u32, sig: i32) -> bool {
    // SAFETY: kill(2) only takes integers.
    unsafe { kill(pid as c_int, sig) == 0 }
}

/// The first real-time signal (Waybar's custom modules listen on RTMIN+N).
pub fn sigrtmin() -> i32 {
    // SAFETY: no arguments.
    unsafe { __libc_current_sigrtmin() }
}

/// True once our parent is gone (we were reparented to init).
pub fn orphaned() -> bool {
    // SAFETY: no arguments.
    unsafe { getppid() == 1 }
}

/// True when the reading end of our stdout has closed.
pub fn stdout_closed() -> bool {
    let mut fd = PollFd {
        fd: 1,
        events: 0,
        revents: 0,
    };
    // SAFETY: one valid PollFd and a zero timeout.
    unsafe { poll(&mut fd, 1, 0) > 0 && fd.revents & (POLLERR | POLLHUP) != 0 }
}

/// Take an exclusive lock on an open file without waiting. The lock lasts as
/// long as the file stays open, so keep it.
pub fn try_lock(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    // SAFETY: a valid descriptor owned by `file`.
    unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) == 0 }
}

/// Local wall-clock time: (weekday 0 = Sunday, hour, minute).
pub fn local_time() -> Option<(u32, u32, u32)> {
    // SAFETY: tm is filled in by localtime_r before it's read.
    unsafe {
        tzset();
        let t = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&t, &mut tm).is_null() {
            return None;
        }
        Some((tm.tm_wday as u32, tm.tm_hour as u32, tm.tm_min as u32))
    }
}

/// A moment as a local calendar date: (year, month 1-12, day 1-31).
pub fn local_date(epoch_seconds: i64) -> Option<(i32, u32, u32)> {
    // SAFETY: as in local_time.
    unsafe {
        tzset();
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&epoch_seconds, &mut tm).is_null() {
            return None;
        }
        Some((tm.tm_year + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32))
    }
}

/// Seconds since 1970 for a local wall-clock time (C's mktime: the local zone
/// and its daylight saving decide the offset).
pub fn local_to_epoch(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Option<i64> {
    // SAFETY: tm is zeroed then filled; mktime normalises it in place.
    unsafe {
        tzset();
        let mut tm: Tm = std::mem::zeroed();
        tm.tm_year = year - 1900;
        tm.tm_mon = month as c_int - 1;
        tm.tm_mday = day as c_int;
        tm.tm_hour = hour as c_int;
        tm.tm_min = min as c_int;
        tm.tm_sec = sec as c_int;
        tm.tm_isdst = -1;
        let t = mktime(&mut tm);
        (t != -1).then_some(t)
    }
}
