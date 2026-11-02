// ── Contenedores Ligeros, Namespaces y Cgroups ─────────────────────────────
extern crate alloc;
use crate::arch::Spinlock;

#[derive(Clone, Debug)]
pub struct ContainerInfo {
    pub id: u32,
    pub name: alloc::string::String,
    pub pid_ns: u32,
    pub mount_ns: u32,
    pub net_ns: u32,
    pub memory_limit: u64,
    pub memory_current: u64,
    pub cpu_quota_pct: u32,
    pub is_running: bool,
}

static CONTAINER_TABLE: Spinlock<alloc::vec::Vec<ContainerInfo>> = Spinlock::new(alloc::vec::Vec::new());

pub fn create_container(name: &str, mem_limit: u64, cpu_quota: u32) -> u32 {
    let mut table = CONTAINER_TABLE.lock();
    let id = (table.len() as u32) + 1;
    table.push(ContainerInfo {
        id,
        name: alloc::string::String::from(name),
        pid_ns: 100 + id,
        mount_ns: 200 + id,
        net_ns: 300 + id,
        memory_limit: mem_limit,
        memory_current: 0,
        cpu_quota_pct: cpu_quota,
        is_running: true,
    });
    id
}

pub fn list_containers() -> alloc::vec::Vec<ContainerInfo> {
    let mut table = CONTAINER_TABLE.lock();
    if table.is_empty() {
        table.push(ContainerInfo {
            id: 1,
            name: alloc::string::String::from("sandbox-default"),
            pid_ns: 101,
            mount_ns: 201,
            net_ns: 301,
            memory_limit: 16 * 1024 * 1024,
            memory_current: 2 * 1024 * 1024,
            cpu_quota_pct: 50,
            is_running: true,
        });
    }
    table.clone()
}

pub fn stop_container(id: u32) -> bool {
    let mut table = CONTAINER_TABLE.lock();
    if let Some(c) = table.iter_mut().find(|c| c.id == id) {
        c.is_running = false;
        true
    } else {
        false
    }
}
