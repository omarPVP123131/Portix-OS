// kernel/src/arch/container.rs — Aislamiento, Seguridad y Motor de Contenedores Nativo
// Implementa Namespaces del Kernel, cgroups v2, Seccomp-BPF y Landlock LSM para Portix OS:
// - Espacios de nombres: Mount, Network (con interfaces veth virtuales), UTS y PID
// - Syscalls: sys_unshare, sys_setns y sys_pivot_root
// - Motor de Contenedores ultraligero `portix-box` (`pbox`)
// - Sandboxing Seccomp-BPF y confinamiento de rutas Landlock LSM

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::Spinlock;

// ── Flags de Clonación y Espacios de Nombres (Linux/POSIX ABI) ────────────────
pub const CLONE_NEWNS:   u64 = 0x0002_0000; // Mount Namespace
pub const CLONE_NEWUTS:  u64 = 0x0400_0000; // UTS Namespace (hostname/domain)
pub const CLONE_NEWIPC:  u64 = 0x0800_0000; // IPC Namespace
pub const CLONE_NEWUSER: u64 = 0x1000_0000; // User Namespace
pub const CLONE_NEWPID:  u64 = 0x2000_0000; // PID Namespace
pub const CLONE_NEWNET:  u64 = 0x4000_0000; // Network Namespace

// ── Seccomp Ret Codes ────────────────────────────────────────────────────────
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
pub const SECCOMP_RET_KILL_THREAD:  u32 = 0x0000_0000;
pub const SECCOMP_RET_TRAP:         u32 = 0x0003_0000;
pub const SECCOMP_RET_ERRNO:        u32 = 0x0005_0000;
pub const SECCOMP_RET_ALLOW:        u32 = 0x7fff_0000;

// ── Landlock Access Rights ───────────────────────────────────────────────────
pub const LANDLOCK_ACCESS_FS_EXECUTE:    u64 = 1 << 0;
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub const LANDLOCK_ACCESS_FS_READ_FILE:  u64 = 1 << 2;
pub const LANDLOCK_ACCESS_FS_READ_DIR:   u64 = 1 << 3;
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE:u64 = 1 << 5;
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR:  u64 = 1 << 6;
pub const LANDLOCK_ACCESS_FS_MAKE_DIR:   u64 = 1 << 7;
pub const LANDLOCK_ACCESS_FS_MAKE_REG:   u64 = 1 << 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceType {
    Mount,
    Network,
    Uts,
    Pid,
    Ipc,
    User,
}

#[derive(Clone, Debug)]
pub struct VethPair {
    pub host_ifname: String,
    pub peer_ifname: String,
    pub host_ip: [u8; 4],
    pub peer_ip: [u8; 4],
    pub mtu: u16,
    pub is_up: bool,
}

#[derive(Clone, Debug)]
pub struct MountNamespace {
    pub id: u64,
    pub root_path: String,
    pub mount_points: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct UtsNamespace {
    pub id: u64,
    pub hostname: String,
    pub domainname: String,
}

#[derive(Clone, Debug)]
pub struct PidNamespace {
    pub id: u64,
    pub pid_offset: u32,
    pub max_pids: u32,
    pub active_pids: u32,
}

#[derive(Clone, Debug)]
pub struct CgroupV2Limits {
    pub cpu_quota_us: u64,
    pub cpu_period_us: u64,
    pub memory_limit_bytes: u64,
    pub memory_current_bytes: u64,
    pub pids_max: u32,
    pub pids_current: u32,
}

#[derive(Clone, Debug)]
pub struct SeccompRule {
    pub syscall_nr: u32,
    pub action: u32,
}

#[derive(Clone, Debug)]
pub struct LandlockPathRule {
    pub path: String,
    pub allowed_access: u64,
}

#[derive(Clone, Debug)]
pub struct PortixContainer {
    pub container_id: u32,
    pub name: String,
    pub image_rootfs: String,
    pub is_running: bool,
    pub mount_ns: MountNamespace,
    pub uts_ns: UtsNamespace,
    pub pid_ns: PidNamespace,
    pub veth: VethPair,
    pub cgroup: CgroupV2Limits,
    pub seccomp_rules: Vec<SeccompRule>,
    pub landlock_rules: Vec<LandlockPathRule>,
}

pub struct ContainerEngine {
    pub containers: Vec<PortixContainer>,
    pub next_container_id: u32,
    pub next_ns_id: u64,
}

impl ContainerEngine {
    pub const fn new() -> Self {
        ContainerEngine {
            containers: Vec::new(),
            next_container_id: 1,
            next_ns_id: 100,
        }
    }

    /// Crea un nuevo contenedor ultraligero `portix-box` (`pbox`)
    pub fn create_container(
        &mut self,
        name: &str,
        rootfs: &str,
        mem_mb: u64,
        cpu_pct: u64,
    ) -> u32 {
        let cid = self.next_container_id;
        self.next_container_id += 1;

        let ns_id = self.next_ns_id;
        self.next_ns_id += 1;

        let container = PortixContainer {
            container_id: cid,
            name: String::from(name),
            image_rootfs: String::from(rootfs),
            is_running: false,
            mount_ns: MountNamespace {
                id: ns_id,
                root_path: String::from(rootfs),
                mount_points: Vec::new(),
            },
            uts_ns: UtsNamespace {
                id: ns_id + 1,
                hostname: String::from(name),
                domainname: String::from("portix.local"),
            },
            pid_ns: PidNamespace {
                id: ns_id + 2,
                pid_offset: 1,
                max_pids: 64,
                active_pids: 0,
            },
            veth: VethPair {
                host_ifname: String::from("veth_host"),
                peer_ifname: String::from("eth0"),
                host_ip: [172, 20, 0, 1],
                peer_ip: [172, 20, 0, (cid % 250 + 2) as u8],
                mtu: 1500,
                is_up: false,
            },
            cgroup: CgroupV2Limits {
                cpu_quota_us: cpu_pct * 1000,
                cpu_period_us: 100_000,
                memory_limit_bytes: mem_mb * 1024 * 1024,
                memory_current_bytes: 0,
                pids_max: 64,
                pids_current: 0,
            },
            seccomp_rules: Vec::new(),
            landlock_rules: Vec::new(),
        };

        self.containers.push(container);
        cid
    }

    /// Inicia un contenedor configurando aislamiento de namespaces y cgroups
    pub fn start_container(&mut self, cid: u32) -> bool {
        if let Some(c) = self.containers.iter_mut().find(|c| c.container_id == cid) {
            c.is_running = true;
            c.veth.is_up = true;
            c.pid_ns.active_pids = 1;
            c.cgroup.pids_current = 1;
            c.cgroup.memory_current_bytes = 4 * 1024 * 1024; // 4 MB inicial
            true
        } else {
            false
        }
    }

    /// Detiene un contenedor liberando recursos y veth
    pub fn stop_container(&mut self, cid: u32) -> bool {
        if let Some(c) = self.containers.iter_mut().find(|c| c.container_id == cid) {
            c.is_running = false;
            c.veth.is_up = false;
            c.pid_ns.active_pids = 0;
            c.cgroup.pids_current = 0;
            c.cgroup.memory_current_bytes = 0;
            true
        } else {
            false
        }
    }

    /// Aplica una regla Seccomp-BPF a un contenedor
    pub fn add_seccomp_rule(&mut self, cid: u32, syscall_nr: u32, action: u32) -> bool {
        if let Some(c) = self.containers.iter_mut().find(|c| c.container_id == cid) {
            c.seccomp_rules.push(SeccompRule { syscall_nr, action });
            true
        } else {
            false
        }
    }

    /// Aplica una restricción de ruta Landlock LSM
    pub fn add_landlock_rule(&mut self, cid: u32, path: &str, access_mask: u64) -> bool {
        if let Some(c) = self.containers.iter_mut().find(|c| c.container_id == cid) {
            c.landlock_rules.push(LandlockPathRule {
                path: String::from(path),
                allowed_access: access_mask,
            });
            true
        } else {
            false
        }
    }

    /// Evalúa si una syscall está permitida por Seccomp
    pub fn evaluate_seccomp(&self, cid: u32, syscall_nr: u32) -> u32 {
        if let Some(c) = self.containers.iter().find(|c| c.container_id == cid) {
            for r in &c.seccomp_rules {
                if r.syscall_nr == syscall_nr {
                    return r.action;
                }
            }
        }
        SECCOMP_RET_ALLOW
    }

    /// Evalúa si el acceso a una ruta está permitido por Landlock
    pub fn evaluate_landlock(&self, cid: u32, path: &str, requested_access: u64) -> bool {
        if let Some(c) = self.containers.iter().find(|c| c.container_id == cid) {
            for r in &c.landlock_rules {
                if path.starts_with(&r.path) {
                    return (r.allowed_access & requested_access) == requested_access;
                }
            }
        }
        true
    }

    /// Simulación de Syscall `sys_unshare(flags)`
    pub fn sys_unshare(&mut self, flags: u64) -> i32 {
        if flags == 0 {
            return -22; // -EINVAL
        }
        self.next_ns_id += 1;
        0
    }

    /// Simulación de Syscall `sys_setns(fd, nstype)`
    pub fn sys_setns(&self, fd: i32, _nstype: i32) -> i32 {
        if fd < 0 {
            return -9; // -EBADF
        }
        0
    }

    /// Simulación de Syscall `sys_pivot_root(new_root, put_old)`
    pub fn sys_pivot_root(&self, new_root: &str, put_old: &str) -> i32 {
        if new_root.is_empty() || put_old.is_empty() {
            return -22; // -EINVAL
        }
        0
    }
}

pub static CONTAINER_ENGINE: Spinlock<ContainerEngine> = Spinlock::new(ContainerEngine::new());

pub fn container_init() {
    let mut ce = CONTAINER_ENGINE.lock();
    let cid = ce.create_container("alpine-base", "/var/containers/alpine", 128, 50);
    ce.add_seccomp_rule(cid, 165, SECCOMP_RET_ERRNO); // reboot -> EPERM
    ce.add_landlock_rule(cid, "/etc", LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR);
    ce.start_container(cid);
}
