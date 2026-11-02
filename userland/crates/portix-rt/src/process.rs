use crate::syscall;

pub fn getpid() -> u64 {
    syscall::sys_getpid()
}

pub fn getppid() -> u64 {
    syscall::sys_getppid()
}

pub fn uname(buf: &mut syscall::UtsName) -> i64 {
    syscall::sys_uname(buf as *mut syscall::UtsName)
}

pub fn exit(code: i32) -> ! {
    syscall::sys_exit(code)
}

pub fn yield_now() {
    syscall::sys_yield()
}

pub fn sleep(ticks: u64) {
    syscall::sys_sleep(ticks)
}

pub fn uptime() -> u64 {
    syscall::sys_uptime()
}

pub const WNOHANG: i32 = 1;
/// [CICLO v6.x] `WUNTRACED`: reportar también los hijos DETENIDOS (job control).
pub const WUNTRACED: i32 = 2;
pub const WCONTINUED: i32 = 8;

// ── Descodificación del estado devuelto por waitpid (estilo POSIX) ──────────
/// Salida normal del hijo.
pub fn wifexited(status: i32) -> bool { (status & 0x7f) == 0 }
/// Código de salida de un hijo terminado.
pub fn wexitstatus(status: i32) -> i32 { (status >> 8) & 0xff }
/// El hijo terminó por señal (no por salida voluntaria).
pub fn wifsignaled(status: i32) -> bool { (((status & 0x7f) + 1) >> 1) > 0 }
pub fn wtermsig(status: i32) -> i32 { status & 0x7f }
/// El hijo está DETENIDO (parada de depuración o de job control).
pub fn wifstopped(status: i32) -> bool { (status & 0xff) == 0x7f }
/// Señal que provocó la parada del hijo (SIGTRAP = 5 en una parada de depuración).
pub fn wstopsig(status: i32) -> i32 { (status >> 8) & 0xff }

pub fn fork() -> i64 {
    syscall::sys_fork()
}


pub fn waitpid(pid: i32, status: &mut i32, options: i32) -> i64 {
    loop {
        let ret = syscall::sys_waitpid(pid, status as *mut i32, options);
        if ret != 0 || (options & WNOHANG) != 0 {
            return ret;
        }
        yield_now();
    }
}

pub fn dup2(oldfd: i32, newfd: i32) -> i64 {
    syscall::sys_dup2(oldfd, newfd)
}

pub fn kill(pid: i32, sig: i32) -> i64 {
    syscall::sys_kill(pid, sig)
}

pub fn getuid() -> u32 {
    unsafe { crate::syscall!(syscall::SYS_GETUID) as u32 }
}

pub fn setuid(uid: u32) -> i32 {
    unsafe { crate::syscall!(syscall::SYS_SETUID, uid as u64) as i32 }
}

pub fn getgid() -> u32 {
    unsafe { crate::syscall!(syscall::SYS_GETGID) as u32 }
}

pub fn setgid(gid: u32) -> i32 {
    unsafe { crate::syscall!(syscall::SYS_SETGID, gid as u64) as i32 }
}

pub fn geteuid() -> u32 {
    syscall::sys_geteuid() as u32
}

pub fn getegid() -> u32 {
    syscall::sys_getegid() as u32
}

pub fn seteuid(euid: u32) -> Result<(), i64> {
    let ret = syscall::sys_seteuid(euid);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn setegid(egid: u32) -> Result<(), i64> {
    let ret = syscall::sys_setegid(egid);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn nanosleep(req: &syscall::Timespec, mut rem: Option<&mut syscall::Timespec>) -> Result<(), i64> {
    let rem_ptr = match rem.as_deref_mut() {
        Some(r) => r as *mut syscall::Timespec,
        None => core::ptr::null_mut(),
    };
    let ret = syscall::sys_nanosleep(req as *const syscall::Timespec, rem_ptr);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn getpgrp() -> u64 {
    syscall::sys_getpgrp()
}

pub fn setpgid(pid: u64, pgid: u64) -> Result<(), i64> {
    let ret = syscall::sys_setpgid(pid, pgid);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn getpgid(pid: u64) -> Result<u64, i64> {
    let ret = syscall::sys_getpgid(pid);
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(ret)
    }
}

pub fn setsid() -> Result<u64, i64> {
    let ret = syscall::sys_setsid();
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(ret)
    }
}

pub fn getsid(pid: u64) -> Result<u64, i64> {
    let ret = syscall::sys_getsid(pid);
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(ret)
    }
}

pub fn poll(fds: &mut [syscall::PollFd], timeout_ms: i32) -> Result<usize, i64> {
    let ret = syscall::sys_poll(fds.as_mut_ptr(), fds.len(), timeout_ms);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret as i64)
    }
}

pub fn clock_gettime(clk_id: i32, tp: &mut syscall::Timespec) -> Result<(), i64> {
    let ret = syscall::sys_clock_gettime(clk_id, tp as *mut syscall::Timespec);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn clock_getres(clk_id: i32, res: &mut syscall::Timespec) -> Result<(), i64> {
    let ret = syscall::sys_clock_getres(clk_id, res as *mut syscall::Timespec);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

/// [Épica 58] Instala un manejador de señales de usuario.
///
/// El manejador recibe el número de señal como primer argumento y puede
/// retornar normalmente: el kernel ha colocado el trampolín de sigreturn como
/// dirección de retorno, de modo que el contexto interrumpido se restaura de
/// forma automática.
pub fn install_handler(signum: i32, handler: extern "C" fn(u64)) -> Result<(), i64> {
    let act = syscall::SigAction {
        sa_handler: handler as usize,
        sa_flags: 0,
        sa_mask: 0,
    };
    sigaction(signum, Some(&act), None)
}

/// [Épica 58] Restaura la acción por defecto (SIG_DFL) de una señal.
pub fn sysaction_default(signum: i32) -> Result<(), i64> {
    let act = syscall::SigAction {
        sa_handler: syscall::SIG_DFL,
        sa_flags: 0,
        sa_mask: 0,
    };
    sigaction(signum, Some(&act), None)
}

/// [Épica 58] Consulta el manejador instalado para una señal.
pub fn get_handler(signum: i32) -> Result<usize, i64> {
    let mut old = syscall::SigAction::default();
    sigaction(signum, None, Some(&mut old))?;
    Ok(old.sa_handler)
}

pub fn sigaction(signum: i32, act: Option<&syscall::SigAction>, mut oldact: Option<&mut syscall::SigAction>) -> Result<(), i64> {
    let act_ptr = match act {
        Some(a) => a as *const syscall::SigAction,
        None => core::ptr::null(),
    };
    let oldact_ptr = match oldact.as_deref_mut() {
        Some(oa) => oa as *mut syscall::SigAction,
        None => core::ptr::null_mut(),
    };
    let ret = syscall::sys_sigaction(signum, act_ptr, oldact_ptr);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn sigprocmask(how: i32, set: Option<u64>, mut oldset: Option<&mut u64>) -> Result<(), i64> {
    let set_ptr = match &set {
        Some(s) => s as *const u64,
        None => core::ptr::null(),
    };
    let oldset_ptr = match oldset.as_deref_mut() {
        Some(os) => os as *mut u64,
        None => core::ptr::null_mut(),
    };
    let ret = syscall::sys_sigprocmask(how, set_ptr, oldset_ptr);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn sigpending() -> Result<u64, i64> {
    let mut pend = 0u64;
    let ret = syscall::sys_sigpending(&mut pend);
    if ret == 0 {
        Ok(pend)
    } else {
        Err(ret as i64)
    }
}

pub fn getrlimit(resource: i32, rlim: &mut syscall::Rlimit) -> Result<(), i64> {
    let ret = syscall::sys_getrlimit(resource, rlim as *mut syscall::Rlimit);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn setrlimit(resource: i32, rlim: &syscall::Rlimit) -> Result<(), i64> {
    let ret = syscall::sys_setrlimit(resource, rlim as *const syscall::Rlimit);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn getpriority(which: i32, who: u64) -> Result<i32, i64> {
    let ret = syscall::sys_getpriority(which, who);
    if ret >= -20 && ret <= 19 {
        Ok(ret)
    } else {
        Err(ret as i64)
    }
}

pub fn setpriority(which: i32, who: u64, prio: i32) -> Result<(), i64> {
    let ret = syscall::sys_setpriority(which, who, prio);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn times(buf: &mut syscall::Tms) -> Result<u64, i64> {
    let ret = syscall::sys_times(buf as *mut syscall::Tms);
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(ret)
    }
}

pub fn getgroups(list: &mut [u32]) -> Result<usize, i64> {
    let ret = syscall::sys_getgroups(list.len(), list.as_mut_ptr());
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn setgroups(groups: &[u32]) -> Result<(), i64> {
    let ret = syscall::sys_setgroups(groups.len(), groups.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn pause() -> i64 {
    syscall::sys_pause()
}

pub fn alarm(seconds: u32) -> u32 {
    syscall::sys_alarm(seconds)
}

pub fn sched_yield() -> Result<(), i64> {
    let ret = syscall::sys_sched_yield();
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn getresuid() -> Result<(u32, u32, u32), i64> {
    let mut r = 0u32;
    let mut e = 0u32;
    let mut s = 0u32;
    let ret = syscall::sys_getresuid(&mut r, &mut e, &mut s);
    if ret == 0 {
        Ok((r, e, s))
    } else {
        Err(ret)
    }
}

pub fn setresuid(ruid: u32, euid: u32, suid: u32) -> Result<(), i64> {
    let ret = syscall::sys_setresuid(ruid, euid, suid);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn getresgid() -> Result<(u32, u32, u32), i64> {
    let mut r = 0u32;
    let mut e = 0u32;
    let mut s = 0u32;
    let ret = syscall::sys_getresgid(&mut r, &mut e, &mut s);
    if ret == 0 {
        Ok((r, e, s))
    } else {
        Err(ret)
    }
}

pub fn setresgid(rgid: u32, egid: u32, sgid: u32) -> Result<(), i64> {
    let ret = syscall::sys_setresgid(rgid, egid, sgid);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn mprotect(addr: *mut u8, len: usize, prot: i32) -> Result<(), i64> {
    let ret = syscall::sys_mprotect(addr, len, prot);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn gettid() -> u64 {
    syscall::sys_gettid()
}

pub fn sched_getparam(pid: u64) -> Result<i32, i64> {
    let mut param = syscall::SchedParam::default();
    let ret = syscall::sys_sched_getparam(pid, &mut param);
    if ret == 0 {
        Ok(param.sched_priority)
    } else {
        Err(ret)
    }
}

pub fn sched_setparam(pid: u64, priority: i32) -> Result<(), i64> {
    let param = syscall::SchedParam { sched_priority: priority };
    let ret = syscall::sys_sched_setparam(pid, &param);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn sched_getscheduler(pid: u64) -> Result<i32, i64> {
    let ret = syscall::sys_sched_getscheduler(pid);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn sched_setscheduler(pid: u64, policy: i32, priority: i32) -> Result<(), i64> {
    let param = syscall::SchedParam { sched_priority: priority };
    let ret = syscall::sys_sched_setscheduler(pid, policy, &param);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn prctl_set_name(name: &str) -> Result<(), i64> {
    let mut buf = [0u8; 16];
    let bytes = name.as_bytes();
    let len = bytes.len().min(15);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    let ret = syscall::sys_prctl(syscall::PR_SET_NAME, buf.as_ptr() as u64, 0, 0, 0);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn prctl_get_name() -> Result<[u8; 16], i64> {
    let mut buf = [0u8; 16];
    let ret = syscall::sys_prctl(syscall::PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
    if ret == 0 {
        Ok(buf)
    } else {
        Err(ret)
    }
}

pub fn tgkill(tgid: i32, tid: i32, sig: i32) -> Result<(), i64> {
    let ret = syscall::sys_tgkill(tgid, tid, sig);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn sched_get_priority_max(policy: i32) -> Result<i32, i64> {
    let ret = syscall::sys_sched_get_priority_max(policy);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn sched_get_priority_min(policy: i32) -> Result<i32, i64> {
    let ret = syscall::sys_sched_get_priority_min(policy);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn futex_wait(addr: &u32, val: u32, timeout: Option<&syscall::Timespec>) -> Result<(), i64> {
    let to_ptr = match timeout {
        Some(t) => t as *const syscall::Timespec,
        None => core::ptr::null(),
    };
    let ret = syscall::sys_futex(addr as *const u32 as *mut u32, syscall::FUTEX_WAIT as i32, val, to_ptr, core::ptr::null_mut(), 0);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn futex_wake(addr: &u32, count: u32) -> Result<i64, i64> {
    let ret = syscall::sys_futex(addr as *const u32 as *mut u32, syscall::FUTEX_WAKE as i32, count, core::ptr::null(), core::ptr::null_mut(), 0);
    if ret >= 0 {
        Ok(ret)
    } else {
        Err(ret)
    }
}

pub fn sysinfo() -> Result<syscall::SysInfo, i64> {
    let mut info = syscall::SysInfo::default();
    let ret = syscall::sys_sysinfo(&mut info);
    if ret == 0 {
        Ok(info)
    } else {
        Err(ret)
    }
}

pub fn getrandom(buf: &mut [u8], flags: u32) -> Result<usize, i64> {
    let ret = syscall::sys_getrandom(buf.as_mut_ptr(), buf.len(), flags);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn sethostname(name: &str) -> Result<(), i64> {
    let ret = syscall::sys_sethostname(name.as_ptr(), name.len());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn gethostname() -> Result<[u8; 65], i64> {
    let mut buf = [0u8; 65];
    let ret = syscall::sys_gethostname(buf.as_mut_ptr(), buf.len());
    if ret == 0 {
        Ok(buf)
    } else {
        Err(ret)
    }
}

pub fn gettimeofday() -> Result<syscall::TimeVal, i64> {
    let mut tv = syscall::TimeVal::default();
    let ret = syscall::sys_gettimeofday(&mut tv, core::ptr::null_mut());
    if ret == 0 {
        Ok(tv)
    } else {
        Err(ret)
    }
}

pub fn getitimer(which: i32) -> Result<syscall::ITimerVal, i64> {
    let mut it = syscall::ITimerVal::default();
    let ret = syscall::sys_getitimer(which, &mut it);
    if ret == 0 {
        Ok(it)
    } else {
        Err(ret)
    }
}

pub fn setitimer(which: i32, new_value: &syscall::ITimerVal) -> Result<syscall::ITimerVal, i64> {
    let mut old = syscall::ITimerVal::default();
    let ret = syscall::sys_setitimer(which, new_value, &mut old);
    if ret == 0 {
        Ok(old)
    } else {
        Err(ret)
    }
}

pub fn getrusage(who: i32) -> Result<syscall::RUsage, i64> {
    let mut usage = syscall::RUsage::default();
    let ret = syscall::sys_getrusage(who, &mut usage);
    if ret == 0 {
        Ok(usage)
    } else {
        Err(ret)
    }
}

pub fn prlimit64(pid: i32, resource: i32, new_limit: Option<&syscall::RLimit64>) -> Result<syscall::RLimit64, i64> {
    let mut old = syscall::RLimit64::default();
    let nptr = match new_limit {
        Some(l) => l as *const syscall::RLimit64,
        None => core::ptr::null(),
    };
    let ret = syscall::sys_prlimit64(pid, resource, nptr, &mut old);
    if ret == 0 {
        Ok(old)
    } else {
        Err(ret)
    }
}



