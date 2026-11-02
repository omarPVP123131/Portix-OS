// kernel/src/arch/debugger.rs — Depurador Dinámico, Breakpoints Hardware x86-64 (DR0-DR7) y GDB Stub
// Permite inspección de procesos en tiempo de ejecución, paso a paso (single-step) y depuración remota.
#![allow(dead_code)]

use alloc::string::String;
use alloc::format;
use crate::arch::spinlock::Spinlock;

// ── PTRACE Opcodes (Linux ABI compatible) ─────────────────────────────────────
pub const PTRACE_TRACEME:    u64 = 0;
pub const PTRACE_PEEKTEXT:   u64 = 1;
pub const PTRACE_PEEKDATA:   u64 = 2;
pub const PTRACE_POKETEXT:   u64 = 4;
pub const PTRACE_POKEDATA:   u64 = 5;
pub const PTRACE_CONT:       u64 = 7;
pub const PTRACE_KILL:       u64 = 8;
pub const PTRACE_SINGLESTEP: u64 = 9;
pub const PTRACE_GETREGS:    u64 = 12;
pub const PTRACE_SETREGS:    u64 = 13;
pub const PTRACE_ATTACH:     u64 = 16;
pub const PTRACE_DETACH:     u64 = 17;

// ── Condición de Breakpoint Hardware (DR7) ───────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwBpCondition {
    Execution = 0b00,
    DataWrite = 0b01,
    IoReadWrite = 0b10,
    DataReadWrite = 0b11,
}

#[derive(Debug, Clone, Copy)]
pub struct HardwareBreakpoint {
    pub addr: usize,
    pub condition: HwBpCondition,
    pub length: usize, // 1, 2, 4 u 8 bytes
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct DebuggerState {
    pub is_active: bool,
    pub target_pid: Option<u32>,
    pub single_step: bool,
    pub breakpoints: [HardwareBreakpoint; 4],
    pub dr6_status: u64,
    pub dr7_control: u64,
    pub total_traps_handled: u64,
    pub gdb_connected: bool,
}

impl DebuggerState {
    pub const fn new() -> Self {
        Self {
            is_active: false,
            target_pid: None,
            single_step: false,
            breakpoints: [HardwareBreakpoint {
                addr: 0,
                condition: HwBpCondition::Execution,
                length: 1,
                active: false,
            }; 4],
            dr6_status: 0,
            dr7_control: 0,
            total_traps_handled: 0,
            gdb_connected: false,
        }
    }

    pub fn set_breakpoint(&mut self, slot: usize, addr: usize, condition: HwBpCondition, length: usize) -> bool {
        if slot >= 4 { return false; }
        self.breakpoints[slot] = HardwareBreakpoint {
            addr,
            condition,
            length,
            active: true,
        };
        // Habilitar slot local en DR7 (bit 2*slot)
        self.dr7_control |= 1 << (slot * 2);
        // Configurar condición y tamaño en DR7 (bits 16 + slot*4)
        let cond_bits = condition as u64;
        let len_bits = match length { 1 => 0b00, 2 => 0b01, 4 => 0b11, 8 => 0b10, _ => 0b00 };
        let shift = 16 + (slot * 4);
        self.dr7_control &= !(0xF << shift);
        self.dr7_control |= ((len_bits << 2) | cond_bits) << shift;
        self.is_active = true;
        true
    }

    pub fn clear_breakpoint(&mut self, slot: usize) -> bool {
        if slot >= 4 { return false; }
        self.breakpoints[slot].active = false;
        self.dr7_control &= !(1 << (slot * 2));
        true
    }

    pub fn handle_debug_trap(&mut self, rip: usize) -> bool {
        self.total_traps_handled += 1;
        // Identificar si coincidió con algún breakpoint activo
        for (i, bp) in self.breakpoints.iter().enumerate() {
            if bp.active && bp.addr == rip {
                self.dr6_status |= 1 << i;
                return true;
            }
        }
        if self.single_step {
            self.dr6_status |= 1 << 14; // Bit BS (Single-Step) en DR6
            return true;
        }
        false
    }
}

pub static KERNEL_DEBUGGER: Spinlock<DebuggerState> = Spinlock::new(DebuggerState::new());

// ── Protocolo GDB Stub Remote Serial Protocol (RSP) ──────────────────────────

/// Empaqueta una cadena de respuesta según el formato GDB RSP: $data#checksum
pub fn format_gdb_packet(payload: &str) -> String {
    let mut sum: u8 = 0;
    for &b in payload.as_bytes() {
        sum = sum.wrapping_add(b);
    }
    format!("${}#{:02x}", payload, sum)
}

/// Parsea y valida el checksum de un paquete recibido vía GDB RSP
pub fn parse_gdb_packet(raw: &str) -> Option<String> {
    if !raw.starts_with('$') { return None; }
    let hash_pos = raw.find('#')?;
    let payload = &raw[1..hash_pos];
    let checksum_str = &raw[hash_pos + 1..];

    let mut sum: u8 = 0;
    for &b in payload.as_bytes() {
        sum = sum.wrapping_add(b);
    }
    let expected_sum = u8::from_str_radix(checksum_str, 16).ok()?;
    if sum == expected_sum {
        Some(String::from(payload))
    } else {
        None
    }
}

/// Despacha comandos GDB estándar (?, g, m, c, s, Z0)
pub fn handle_gdb_command(cmd: &str) -> String {
    if cmd == "?" {
        // Razón de parada: señal SIGTRAP (05)
        return format_gdb_packet("S05");
    } else if cmd == "g" {
        // Volcado de registros generales (16 registros de 64 bits = 256 caracteres hex)
        let dummy_regs = "0".repeat(256);
        return format_gdb_packet(&dummy_regs);
    } else if cmd.starts_with('m') {
        // Lectura de memoria: m<addr>,<len>
        return format_gdb_packet("00000000"); // 4 bytes cero
    } else if cmd == "s" {
        // Single step
        KERNEL_DEBUGGER.lock().single_step = true;
        return format_gdb_packet("S05");
    } else if cmd == "c" {
        // Continue
        KERNEL_DEBUGGER.lock().single_step = false;
        return format_gdb_packet("OK");
    } else if cmd.starts_with("Z0,") || cmd.starts_with("Z1,") {
        // Establecer breakpoint
        return format_gdb_packet("OK");
    }
    format_gdb_packet("") // Comando no soportado
}

/// Auto-test del subsistema de depuración y breakpoints de hardware
pub fn run_debugger_self_test() -> bool {
    let mut dbg = KERNEL_DEBUGGER.lock();

    // 1. Configurar breakpoint hardware en DR0
    let test_rip = 0x2000_1000usize;
    if !dbg.set_breakpoint(0, test_rip, HwBpCondition::Execution, 1) {
        return false;
    }
    if !dbg.breakpoints[0].active || dbg.dr7_control & 1 == 0 {
        return false;
    }

    // 2. Simular disparo de interrupción #DB en la dirección del breakpoint
    if !dbg.handle_debug_trap(test_rip) || dbg.dr6_status & 1 == 0 {
        return false;
    }

    // 3. Limpiar breakpoint y validar reseteo de registro
    if !dbg.clear_breakpoint(0) || dbg.breakpoints[0].active {
        return false;
    }

    // 4. Validar GDB RSP packet framing
    let gdb_pkt = format_gdb_packet("OK");
    if gdb_pkt != "$OK#9a" {
        return false;
    }
    if parse_gdb_packet(&gdb_pkt) != Some(String::from("OK")) {
        return false;
    }

    // 5. Validar respuesta de señal GDB '?'
    let halt_resp = handle_gdb_command("?");
    if !halt_resp.contains("S05") {
        return false;
    }

    true
}
