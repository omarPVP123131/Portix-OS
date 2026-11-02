use crate::arch::Spinlock;
use crate::drivers::serial;
use crate::process::{self, MAX_PROCS, ProcessState};

pub const IPC_DATA_SIZE: usize = 40;
pub const IPC_QUEUE_CAPACITY: usize = 16;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct IpcMessage {
    pub src_pid: u64,
    pub dst_pid: u64,
    pub msg_type: u64,
    pub data: [u8; IPC_DATA_SIZE],
}

#[derive(Copy, Clone)]
struct PerProcQueue {
    msgs: [IpcMessage; IPC_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

const ZERO_MSG: IpcMessage = IpcMessage {
    src_pid: 0,
    dst_pid: 0,
    msg_type: 0,
    data: [0; IPC_DATA_SIZE],
};

const INIT_QUEUE: PerProcQueue = PerProcQueue {
    msgs: [ZERO_MSG; IPC_QUEUE_CAPACITY],
    head: 0,
    tail: 0,
    count: 0,
};

const MAX_IRQ: usize = 16;

struct IpcState {
    mailboxes: [PerProcQueue; MAX_PROCS],
    irq_routes: [Option<u64>; MAX_IRQ],
}

impl IpcState {
    const fn new() -> Self {
        IpcState {
            mailboxes: [INIT_QUEUE; MAX_PROCS],
            irq_routes: [None; MAX_IRQ],
        }
    }

    fn cleanup_process(&mut self, slot: usize) {
        if slot < MAX_PROCS {
            self.mailboxes[slot] = INIT_QUEUE;
        }
    }

    fn send(&mut self, src_pid: u64, dst_pid: u64, msg_type: u64, data: &[u8]) -> i64 {
        let dst_slot = match process::pid_to_slot(dst_pid) {
            Some(s) => s,
            None => {
                serial::write_str("[IPC] SEND: dst PID not found\n");
                return -3; // -ESRCH
            }
        };

        let msg = &mut self.mailboxes[dst_slot];
        if msg.count >= IPC_QUEUE_CAPACITY {
            serial::write_str("[IPC] SEND: queue full\n");
            return -11; // -EAGAIN
        }

        let mut payload = [0u8; IPC_DATA_SIZE];
        let copy_len = data.len().min(IPC_DATA_SIZE);
        payload[..copy_len].copy_from_slice(&data[..copy_len]);

        msg.msgs[msg.tail] = IpcMessage {
            src_pid, dst_pid, msg_type, data: payload,
        };
        msg.tail = (msg.tail + 1) % IPC_QUEUE_CAPACITY;
        msg.count += 1;

        serial::write_str("[IPC] PID ");
        serial::write_usize(src_pid as usize);
        serial::write_str(" -> PID ");
        serial::write_usize(dst_pid as usize);
        serial::write_str(": msg type=");
        serial::write_usize(msg_type as usize);
        serial::write_str(" size=");
        serial::write_usize(copy_len);
        serial::write_str("\n");

        0
    }

    fn recv(&mut self, pid: u64, buf: &mut [u8]) -> i64 {
        let slot = match process::pid_to_slot(pid) {
            Some(s) => s,
            None => return -1,
        };

        let msg = &mut self.mailboxes[slot];
        if msg.count == 0 {
            return 1;
        }

        let ipc_msg = &msg.msgs[msg.head];
        let total_size = 8u64 + 8 + 8 + IPC_DATA_SIZE as u64;
        let copy_len = buf.len().min(total_size as usize);
        buf[..8].copy_from_slice(&ipc_msg.src_pid.to_le_bytes());
        if copy_len > 8 {
            buf[8..16].copy_from_slice(&ipc_msg.dst_pid.to_le_bytes());
        }
        if copy_len > 16 {
            buf[16..24].copy_from_slice(&ipc_msg.msg_type.to_le_bytes());
        }
        if copy_len > 24 {
            let data_copy = (copy_len - 24).min(IPC_DATA_SIZE);
            buf[24..24 + data_copy].copy_from_slice(&ipc_msg.data[..data_copy]);
        }

        msg.head = (msg.head + 1) % IPC_QUEUE_CAPACITY;
        msg.count -= 1;

        serial::write_str("[IPC] PID ");
        serial::write_usize(pid as usize);
        serial::write_str(" recv from PID ");
        serial::write_usize(ipc_msg.src_pid as usize);
        serial::write_str(" type=");
        serial::write_usize(ipc_msg.msg_type as usize);
        serial::write_str(" remaining=");
        serial::write_usize(msg.count);
        serial::write_str("\n");

        0
    }
}

static IPC: Spinlock<IpcState> = Spinlock::new(IpcState::new());

fn slot_of_pid(pid: u64) -> Option<usize> {
    process::pid_to_slot(pid)
}

/// Autocomprobación de ida y vuelta del IPC **sin efectos secundarios**:
/// guarda el estado de la cola del proceso indicado, inyecta un mensaje de
/// prueba, lo recupera verificando su contenido y restaura exactamente el
/// estado previo (cabeza, cola y contador).
pub fn loopback_selftest(pid: u64) -> bool {
    let payload = [0x50u8, 0x49, 0x50, 0x43]; // "PIPC"
    let mut ok = false;
    {
        let mut ipc = IPC.lock();
        let slot = match process::pid_to_slot(pid) {
            Some(s) => s,
            None => return false,
        };
        // Estado previo de la cola (restauración garantizada al final)
        let saved_head = ipc.mailboxes[slot].head;
        let saved_tail = ipc.mailboxes[slot].tail;
        let saved_count = ipc.mailboxes[slot].count;
        let used = saved_count as usize;

        let sent = ipc.send(pid, pid, 0xC0DE, &payload);
        if sent == 0 {
            let mut buf = [0u8; 64];
            let received = ipc.recv(pid, &mut buf);
            ok = received == 0 && buf[24..28] == payload;
        }

        // Restauración del estado previo: la cola no conserva rastro
        ipc.mailboxes[slot].head = saved_head;
        ipc.mailboxes[slot].tail = saved_tail;
        ipc.mailboxes[slot].count = saved_count;
        let _ = used;
    }
    ok
}

/// Estadísticas del subsistema de IPC (consultas sin efectos secundarios).
pub fn stats() -> (usize, usize, usize, u64) {
    let ipc = IPC.lock();
    let mut queued = 0usize;
    let mut busy = 0usize;
    for m in ipc.mailboxes.iter() {
        queued += m.count as usize;
        if m.count > 0 {
            busy += 1;
        }
    }
    let routes = ipc.irq_routes.iter().filter(|r| r.is_some()).count();
    (ipc.mailboxes.len(), queued, busy, routes as u64)
}

pub fn init() {
    serial::write_str("[IPC] system initialized (max ");
    serial::write_usize(MAX_PROCS);
    serial::write_str(" mailboxes, ");
    serial::write_usize(IPC_QUEUE_CAPACITY);
    serial::write_str(" msgs/queue)\n");
}

pub fn cleanup_process(slot: usize) {
    if let Some(mut guard) = IPC.try_lock() {
        guard.cleanup_process(slot);
    } else {
        serial::write_str("[IPC] cleanup_process: lock busy, skipping\n");
    }
}

pub fn send(src_pid: u64, dst_pid: u64, msg_type: u64, data: &[u8]) -> i64 {
    let res = IPC.lock().send(src_pid, dst_pid, msg_type, data);
    // Wake destination outside IPC lock to avoid deadlock with process lock
    if res == 0 {
        if let Some(proc) = process::process_by_pid(dst_pid) {
            if proc.state == ProcessState::Blocked {
                proc.state = ProcessState::Ready;
                proc.sleep_until = 0;
                serial::write_str("[IPC] wake PID ");
                serial::write_usize(dst_pid as usize);
                serial::write_str("\n");
            }
        }
    }
    res
}

pub fn recv(pid: u64, buf: &mut [u8]) -> i64 {
    IPC.lock().recv(pid, buf)
}

pub fn register_irq(irq: usize, pid: u64) -> i64 {
    if irq >= MAX_IRQ {
        serial::write_str("[IPC] REG_IRQ: invalid IRQ ");
        serial::write_usize(irq);
        serial::write_str("\n");
        return -22; // -EINVAL
    }
    IPC.lock().irq_routes[irq] = Some(pid);
    serial::write_str("[IPC] IRQ");
    serial::write_usize(irq);
    serial::write_str(" -> PID ");
    serial::write_usize(pid as usize);
    serial::write_str("\n");
    0
}

pub fn notify_irq(irq: usize) {
    if irq >= MAX_IRQ { return; }
    let data = [irq as u8; IPC_DATA_SIZE];
    let mut guard = IPC.lock();
    if let Some(dst_pid) = guard.irq_routes[irq] {
        serial::write_str("[IPC] IRQ");
        serial::write_usize(irq);
        serial::write_str(" -> PID ");
        serial::write_usize(dst_pid as usize);
        serial::write_str("\n");
        let _ = guard.send(0, dst_pid, 0xFF, &data);
    }
}

fn irq_routed(irq: u64) -> u8 {
    if (irq as usize) < MAX_IRQ {
        if IPC.try_lock().map_or(false, |g| g.irq_routes[irq as usize].is_some()) { 1 } else { 0 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ipc_notify_irq_handler(irq: u64) {
    notify_irq(irq as usize);
}

#[no_mangle]
pub extern "C" fn is_irq_routed(irq: u64) -> u8 {
    irq_routed(irq)
}
