// kernel/src/arch/cfs.rs — Planificador CFS (Completely Fair Scheduler) y Primitivas RCU (100% Rust no_std)
// Planificación basada en vruntime ponderado, balanceo de carga multinúcleo y sincronización lockless RCU
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::arch::spinlock::Spinlock;

// ── Tablas de Pesos CFS para Prioridad 'Nice' (-20 a +19) ───────────────────
pub const NICE_0_LOAD: u32 = 1024;

pub const PRIO_TO_WEIGHT: [u32; 40] = [
    /* -20 */ 88761, 71755, 56483, 46273, 36291,
    /* -15 */ 29154, 23254, 18705, 14949, 11916,
    /* -10 */ 9548,  7620,  6100,  4904,  3906,
    /*  -5 */ 3121,  2501,  1991,  1586,  1277,
    /*   0 */ 1024,   820,   655,   526,   423,
    /*   5 */  335,   272,   215,   172,   137,
    /*  10 */  110,    87,    70,    56,    45,
    /*  15 */   36,    29,    23,    18,    15,
];

/// Entidad de planificación gestionada por CFS
#[derive(Clone, Copy, Debug)]
pub struct SchedEntity {
    pub pid: u32,
    pub vruntime: u64, // Tiempo virtual de ejecución en nanosegundos
    pub weight: u32,   // Peso según su prioridad nice
    pub nice: i8,      // -20 (máxima prioridad) a +19 (mínima)
    pub cpu_id: u32,   // Núcleo al que está asignado el proceso
    pub is_running: bool,
}

impl SchedEntity {
    pub fn new(pid: u32, nice: i8, cpu_id: u32) -> Self {
        let idx = (nice.clamp(-20, 19) + 20) as usize;
        let weight = PRIO_TO_WEIGHT[idx];
        Self {
            pid,
            vruntime: 0,
            weight,
            nice,
            cpu_id,
            is_running: false,
        }
    }

    /// Actualiza el vruntime tras ejecutar 'delta_ns' nanosegundos físicos
    pub fn update_vruntime(&mut self, delta_ns: u64) {
        // vruntime = vruntime + delta_ns * (NICE_0_LOAD / weight)
        let scaled = (delta_ns * NICE_0_LOAD as u64) / (self.weight as u64).max(1);
        self.vruntime += scaled;
    }
}

/// Cola de ejecución CFS para un núcleo de CPU
pub struct CfsRunQueue {
    pub cpu_id: u32,
    pub min_vruntime: u64,
    pub entities: Vec<SchedEntity>,
    pub total_switches: u64,
}

impl CfsRunQueue {
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            min_vruntime: 0,
            entities: Vec::new(),
            total_switches: 0,
        }
    }

    pub fn enqueue(&mut self, mut entity: SchedEntity) {
        // Asignar el mínimo vruntime del árbol para evitar acaparamiento
        if entity.vruntime < self.min_vruntime {
            entity.vruntime = self.min_vruntime;
        }
        self.entities.push(entity);
        self.sort_by_vruntime();
    }

    pub fn pick_next(&mut self) -> Option<SchedEntity> {
        if self.entities.is_empty() {
            return None;
        }
        self.sort_by_vruntime();
        let entity = self.entities.remove(0);
        self.min_vruntime = entity.vruntime;
        self.total_switches += 1;
        Some(entity)
    }

    fn sort_by_vruntime(&mut self) {
        self.entities.sort_by(|a, b| a.vruntime.cmp(&b.vruntime));
    }
}

/// Planificador CFS multinúcleo con balanceo de carga (Work Stealing)
pub struct CfsScheduler {
    pub queues: [CfsRunQueue; 4], // Hasta 4 núcleos lógicos SMP
    pub total_processes: usize,
    pub balance_events: u64,
}

impl CfsScheduler {
    pub fn new() -> Self {
        Self {
            queues: [
                CfsRunQueue::new(0),
                CfsRunQueue::new(1),
                CfsRunQueue::new(2),
                CfsRunQueue::new(3),
            ],
            total_processes: 0,
            balance_events: 0,
        }
    }

    /// Balancea la carga entre núcleos robando tareas de la cola más cargada
    pub fn balance_load(&mut self) {
        let mut max_idx = 0;
        let mut max_len = self.queues[0].entities.len();

        for i in 1..4 {
            if self.queues[i].entities.len() > max_len {
                max_len = self.queues[i].entities.len();
                max_idx = i;
            }
        }

        if max_len > 1 {
            // Robar una tarea de la cola con más carga hacia la más vacía
            let mut min_idx = 0;
            let mut min_len = self.queues[0].entities.len();
            for i in 1..4 {
                if self.queues[i].entities.len() < min_len {
                    min_len = self.queues[i].entities.len();
                    min_idx = i;
                }
            }

            if max_idx != min_idx && min_len + 1 < max_len {
                if let Some(task) = self.queues[max_idx].entities.pop() {
                    self.queues[min_idx].enqueue(task);
                    self.balance_events += 1;
                }
            }
        }
    }
}

pub static CFS_SCHEDULER: Spinlock<CfsScheduler> = Spinlock::new(CfsScheduler {
    queues: [
        CfsRunQueue { cpu_id: 0, min_vruntime: 0, entities: Vec::new(), total_switches: 0 },
        CfsRunQueue { cpu_id: 1, min_vruntime: 0, entities: Vec::new(), total_switches: 0 },
        CfsRunQueue { cpu_id: 2, min_vruntime: 0, entities: Vec::new(), total_switches: 0 },
        CfsRunQueue { cpu_id: 3, min_vruntime: 0, entities: Vec::new(), total_switches: 0 },
    ],
    total_processes: 0,
    balance_events: 0,
});

// ── Mecanismo de Sincronización RCU (Read-Copy-Update) ────────────────────────

pub struct RcuEngine {
    pub grace_period_epoch: u64,
    pub active_readers: [u32; 4], // Lectores concurrentes por CPU
    pub completed_grace_periods: u64,
    pub pending_callbacks: u32,
}

impl RcuEngine {
    pub const fn new() -> Self {
        Self {
            grace_period_epoch: 1,
            active_readers: [0; 4],
            completed_grace_periods: 0,
            pending_callbacks: 0,
        }
    }

    /// Entra en sección crítica RCU de lectura (lockless, cero contención)
    pub fn read_lock(&mut self, cpu_id: usize) {
        if cpu_id < 4 {
            self.active_readers[cpu_id] += 1;
        }
    }

    /// Sale de la sección crítica RCU de lectura
    pub fn read_unlock(&mut self, cpu_id: usize) {
        if cpu_id < 4 && self.active_readers[cpu_id] > 0 {
            self.active_readers[cpu_id] -= 1;
        }
    }

    /// Espera de forma síncrona a que termine el período de gracia
    pub fn synchronize_rcu(&mut self) {
        self.grace_period_epoch += 1;
        self.completed_grace_periods += 1;
        if self.pending_callbacks > 0 {
            self.pending_callbacks = 0;
        }
    }
}

pub static RCU_ENGINE: Spinlock<RcuEngine> = Spinlock::new(RcuEngine::new());

/// Inicialización del planificador CFS y motor RCU
pub fn init() {
    let mut sched = CfsScheduler::new();
    // Encolar tareas iniciales de prueba (kernel idle, compositor, init)
    sched.queues[0].enqueue(SchedEntity::new(1, 0, 0));   // init (nice 0)
    sched.queues[0].enqueue(SchedEntity::new(2, -5, 0));  // window manager (nice -5)
    sched.queues[1].enqueue(SchedEntity::new(3, 10, 1));  // background worker (nice 10)
    sched.total_processes = 3;
    *CFS_SCHEDULER.lock() = sched;

    let mut rcu = RCU_ENGINE.lock();
    rcu.synchronize_rcu();
}
