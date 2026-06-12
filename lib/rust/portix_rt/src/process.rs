use crate::syscall;

pub fn getpid() -> u64 {
    syscall::sys_getpid()
}

pub fn exit(code: i32) -> ! {
    syscall::sys_exit(code)
}

pub fn yield_cpu() {
    syscall::sys_yield()
}

pub fn sleep(ticks: u64) {
    syscall::sys_sleep(ticks)
}

pub fn uptime() -> u64 {
    syscall::sys_uptime()
}

pub fn dup2(oldfd: i32, newfd: i32) -> i64 {
    syscall::sys_dup2(oldfd, newfd)
}
