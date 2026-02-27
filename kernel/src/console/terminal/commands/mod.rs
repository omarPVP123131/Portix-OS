// console/terminal/commands/mod.rs
// Tabla de dispatch central. Añadir un comando nuevo = 1 línea aquí + función en el módulo correcto.

pub mod system;
pub mod debug;
pub mod convert;
pub mod fun;

use crate::console::terminal::{Terminal, LineColor, INPUT_MAX};

/// Enruta `cmd` al handler correspondiente. Llamado desde `Terminal::enter()`.
pub fn dispatch(
    t:   &mut Terminal,
    cmd: &[u8],
    args: &[u8],
    hw:  &crate::arch::hardware::HardwareInfo,
    pci: &crate::drivers::bus::pci::PciBus,
) {
    match cmd {
        // ── Ayuda ────────────────────────────────────────────────────────────
        b"help" | b"ayuda" | b"?" | b"h"
            => system::cmd_help(t),

        // ── Información del sistema ──────────────────────────────────────────
        b"info"
            => system::cmd_info(t, hw),
        b"cpu" | b"lscpu"
            => system::cmd_cpu(t, hw),
        b"mem" | b"memory" | b"lsmem"
            => system::cmd_mem(t, hw),
        b"disks" | b"storage" | b"lsblk"
            => system::cmd_disks(t, hw),
        b"pci" | b"lspci"
            => system::cmd_pci(t, pci),
        b"neofetch" | b"fetch"
            => system::cmd_neofetch(t, hw, pci),
        b"uname"
            => t.write_line("  PORTIX 0.7 x86_64 bare-metal #1", LineColor::Normal),
        b"whoami"
            => t.write_line("  root", LineColor::Success),
        b"hostname"
            => t.write_line("  portix-kernel", LineColor::Normal),
        b"motd"
            => system::cmd_motd(t),
        b"ver" | b"version"
            => system::cmd_ver(t),
        b"uptime" | b"time"
            => system::cmd_uptime(t),
        b"date" | b"fecha"
            => system::cmd_fecha(t),
        b"ticks"
            => system::cmd_ticks(t),

        // ── Terminal ─────────────────────────────────────────────────────────
        b"clear" | b"cls" | b"limpiar"
            => t.clear_history(),
        b"echo" | b"print"
            => t.write_bytes(args, LineColor::Normal),
        b"history" | b"historial"
            => system::cmd_history(t),

        // ── Cálculo y conversión ─────────────────────────────────────────────
        b"calc" | b"math" | b"="
            => convert::cmd_calc(t, args),
        b"hex"  => convert::cmd_hex(t, args),
        b"dec"  => convert::cmd_dec(t, args),
        b"bin"  => convert::cmd_bin(t, args),
        b"rgb"  => convert::cmd_rgb(t, args),

        // ── Hardware / depuración ────────────────────────────────────────────
        b"hexdump" | b"dump" | b"hd"
            => debug::cmd_hexdump(t, args),
        b"peek"    => debug::cmd_peek(t, args),
        b"poke"    => debug::cmd_poke(t, args),
        b"cpuid"   => debug::cmd_cpuid(t, args),
        b"pic" | b"lsirq"
            => debug::cmd_pic(t),
        b"gdt"     => debug::cmd_gdt(t),
        b"memtest" => debug::cmd_memtest(t, args),
        b"inb"     => debug::cmd_inb(t, args),
        b"outb"    => debug::cmd_outb(t, args),

        // ── Entretenimiento ──────────────────────────────────────────────────
        b"beep"    => fun::cmd_beep(t, args),
        b"colors" | b"palette" | b"colores"
            => fun::cmd_colors(t),
        b"ascii" | b"art"
            => fun::cmd_ascii_art(t),
        b"banner"  => fun::cmd_banner(t, args),
        b"progress"=> fun::cmd_progress(t),
        b"matrix"  => fun::cmd_matrix(t),
        b"scrolltest" | b"scroll"
            => fun::cmd_scrolltest(t),

        // ── Energía ──────────────────────────────────────────────────────────
        b"poweroff" | b"shutdown" | b"apagar" => {
            t.write_line("  Apagando el sistema...", LineColor::Warning);
            crate::drivers::bus::acpi::poweroff();
        }
        b"reboot" | b"restart" | b"reiniciar" => {
            t.write_line("  Reiniciando...", LineColor::Warning);
            crate::drivers::bus::acpi::reboot();
        }

        // ── Comando desconocido ──────────────────────────────────────────────
        _ => {
            let mut buf = [0u8; INPUT_MAX]; let mut pos = 0;
            for b in b"  Error: comando no encontrado: " { buf[pos] = *b; pos += 1; }
            let l = cmd.len().min(40);
            buf[pos..pos + l].copy_from_slice(&cmd[..l]);
            t.write_bytes(&buf[..pos + l], LineColor::Error);
            t.write_line("  Escribe 'help' para ver los comandos disponibles.", LineColor::Normal);
        }
    }
}