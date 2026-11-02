// kernel/src/main.rs — PORTIX Kernel v0.14.0
//
// FIXES:
//   - ctrl leído desde kbd.ctrl() en lugar de hardcoded false
//   - ide_handle_click() integrado para clicks en menubar del IDE
//   - explorer: preview se carga tras mover selección
//   - variables ide/explorer: binding único sin doble-shadow
//   - unused_unsafe: init_page_pool ya es safe (el unsafe está dentro)

#![no_std]
#![no_main]
#![allow(dead_code)]
#![allow(static_mut_refs)]
#![allow(unused_features)]
#![feature(alloc_error_handler)]
#![feature(allocator_ext)]

extern crate alloc;

pub mod arch;
pub mod bootinfo;
pub mod console;
pub mod drivers;
pub mod graphics;
pub mod elf;
pub mod ipc;
pub mod mem;
pub mod fs;
pub mod process;
pub mod syscall;
pub mod time;
pub mod ui;
pub mod util;
pub mod net;
pub mod lkm;
pub mod version;
pub mod wasm;
pub mod crypto;
pub mod microkernel;
pub mod compat;
pub mod power;
pub mod system;
pub mod containers;
pub mod security;
pub mod dev;

use console::terminal::LineColor;
use core::arch::global_asm;
use drivers::input::keyboard::Key;
use alloc::boxed::Box;
use crate::drivers::serial;
use drivers::storage::traits::BlockDevice;
use drivers::storage::ata::DriveType;
use drivers::storage::{ata, atapi, fat32, registry};
use graphics::driver::framebuffer::{Color, Console, Layout};
use mem::allocator::BuddyAllocator;

#[no_mangle]
extern "Rust" fn __rust_alloc_error_handler(size: usize, align: usize) -> ! {
    panic!("OOM: size={} align={}", size, align);
}

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    pub static __stack_top: u8;
}

/// [CICLO 49] Tabla de páginas estática, alineada a 4 KiB, para el mapa de la
/// mitad superior que construye `_start` antes de saltar a la dirección alta.
#[repr(C, align(4096))]
pub struct TablaPaginasBoot(pub [u64; 512]);

/// PDPT de PML4[511] (0xFFFFFF8000000000..): su entrada 510 apunta a `HH_PD`.
#[no_mangle]
pub static mut HH_PDPT: TablaPaginasBoot = TablaPaginasBoot([0; 512]);
/// PD que cubre 0xFFFFFFFF80000000..+1 GiB -> físico 0..1 GiB (512 páginas de 2 MiB).
#[no_mangle]
pub static mut HH_PD: TablaPaginasBoot = TablaPaginasBoot([0; 512]);

// ── Punto de entrada en ensamblador ──────────────────────────────────────────
// El cargador (stage2 BIOS o portix-efi-loader) salta a FÍSICO 0x200000 con
// identidad plana activa. Este código está ENLAZADO en 0xFFFFFFFF80200000, pero
// sólo usa direccionamiento relativo a RIP hasta el salto a la mitad superior,
// así que funciona ejecutándose en la dirección física. Secuencia:
//   1. limpiar .bss (relativo a RIP: direcciones físicas, ya mapeadas)
//   2. HH_PD[i] = i*2MiB | PRESENT|WRITABLE|HUGE  (i = 0..512)
//   3. HH_PDPT[510] = phys(HH_PD) | 3 ; PML4[511] = phys(HH_PDPT) | 3
//      (si PML4[511] ya estuviera ocupada se detiene con "HH!" por serie:
//      nunca se pisa una tabla ajena)
//   4. recarga CR3, salta a `4:` + KERNEL_VMA_OFFSET, fija la pila alta y
//      llama a rust_main.  A partir de aquí RIP/RSP/símbolos son altos.
global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code64",
    "_start:",
    "    cli",
    "    cld",
    "    mov r12, rdi",
    "    xor rbp, rbp",
    // 1. .bss a cero (incluye HH_PDPT/HH_PD)
    "    lea rdi, [rip + {BSS_START}]",
    "    lea rcx, [rip + {BSS_END}]",
    "    sub rcx, rdi",
    "    jz 1f",
    "    test rcx, rcx",
    "    js  1f",
    "    xor eax, eax",
    "    rep stosb",
    "1:",
    // 2. PD de la mitad superior: 1 GiB físico en páginas de 2 MiB
    "    lea rdi, [rip + {HH_PD}]",
    "    mov rax, 0x83",
    "    mov ecx, 512",
    "3:  mov [rdi], rax",
    "    add rax, 0x200000",
    "    add rdi, 8",
    "    dec ecx",
    "    jnz 3b",
    // 3. PDPT[510] -> PD ; PML4[511] -> PDPT (sólo si está libre)
    "    lea rdi, [rip + {HH_PDPT}]",
    "    lea rax, [rip + {HH_PD}]",
    "    or  rax, 3",
    "    mov [rdi + 510*8], rax",
    "    mov rax, cr3",
    "    mov rdx, 0xFFFFFFFFFFFFF000",
    "    and rax, rdx",
    "    mov rdx, [rax + 511*8]",
    "    test rdx, 1",
    "    jnz 9f",
    "    lea rdx, [rip + {HH_PDPT}]",
    "    or  rdx, 3",
    "    mov [rax + 511*8], rdx",
    "    mov rax, cr3",
    "    mov cr3, rax",
    // 4. salto a la mitad superior
    "    lea rax, [rip + 4f]",
    "    mov rdx, {VMA_OFFSET}",
    "    add rax, rdx",
    "    jmp rax",
    "4:",
    "    lea rsp, [rip + {STACK_TOP}]",
    "    xor rbp, rbp",
    "    mov rdi, r12",
    "    call {RUST_MAIN}",
    "2:  hlt",
    "    jmp 2b",
    // PML4[511] ocupada: 'HH!' por COM1 y parada. No se continúa a ciegas.
    "9:  mov dx, 0x3F8",
    "    mov al, 'H'",
    "    out dx, al",
    "    out dx, al",
    "    mov al, '!'",
    "    out dx, al",
    "    mov al, 10",
    "    out dx, al",
    "8:  hlt",
    "    jmp 8b",
    STACK_TOP = sym __stack_top,
    BSS_START = sym __bss_start,
    BSS_END   = sym __bss_end,
    RUST_MAIN = sym rust_main,
    HH_PDPT   = sym HH_PDPT,
    HH_PD     = sym HH_PD,
    VMA_OFFSET = const mem::paging::KERNEL_VMA_OFFSET,
);

#[global_allocator]
static ALLOCATOR: BuddyAllocator = BuddyAllocator::new();

// ── Constantes ────────────────────────────────────────────────────────────────

const RENDER_HZ: u64 = 30;
const RENDER_INTERVAL: u64 = 100 / RENDER_HZ;

unsafe fn init_cpu_features() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack, nomem));
    cr0 &= !(1 << 2); // EM=0
    cr0 |= 1 << 1;    // MP=1
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack, nomem));

    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, nomem));
    cr4 |= (1 << 9) | (1 << 10); // OSFXSR + OSXMMEXCPT
    core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, nomem));

    // [CICLO 50 / T1b.2] NX por página: EFER.NXE sólo si CPUID 0x80000001.EDX[20]
    // lo anuncia. Sin NXE el bit 63 de una PTE es reservado y provocaría #PF.
    let (_a, _b, _c, edx): (u32, u32, u32, u32);
    core::arch::asm!("push rbx; cpuid; mov {b:e}, ebx; pop rbx",
        inout("eax") 0x8000_0001u32 => _a, b = out(reg) _b, out("ecx") _c, out("edx") edx,
        options(nostack));
    if edx & (1 << 20) != 0 {
        let (lo, hi): (u32, u32);
        core::arch::asm!("rdmsr", in("ecx") 0xC000_0080u32, out("eax") lo, out("edx") hi, options(nostack, nomem));
        let lo = lo | (1 << 11);
        core::arch::asm!("wrmsr", in("ecx") 0xC000_0080u32, in("eax") lo, in("edx") hi, options(nostack, nomem));
        mem::paging::activar_nx();
    }
}

// ── [CICLO 49] Autocomprobación de la mitad superior ─────────────────────────
/// Comprueba, nada más entrar en Rust, que el núcleo ejecuta de verdad en la
/// mitad superior y que su mapa es coherente con la física. Si algo falla se
/// imprime `[HH-FALLO]` y se detiene la máquina: nunca se sigue arrancando con
/// un mapa de memoria dudoso.
fn verificar_mitad_superior() {
    use mem::paging::{self, KERNEL_PHYS_BASE, KERNEL_VMA_OFFSET};
    let rip: usize;
    unsafe { core::arch::asm!("lea {}, [rip + 0]", out(reg) rip, options(nomem, nostack)); }
    let rsp: usize;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack)); }
    let main_va = rust_main as *const () as usize;
    let main_pa = paging::virt_to_phys(main_va);
    let cr3 = paging::read_cr3();
    let traducido = paging::translate(cr3, main_va).map(|(p, _)| p);
    let tabla_va = core::ptr::addr_of!(HH_PD) as usize;
    let tabla_ok = paging::translate(cr3, tabla_va).map(|(p, _)| p) == Some(paging::virt_to_phys(tabla_va));
    let ok = rip >= KERNEL_VMA_OFFSET
        && rsp >= KERNEL_VMA_OFFSET
        && main_pa >= KERNEL_PHYS_BASE && main_pa < KERNEL_PHYS_BASE + 0x40_0000
        && traducido == Some(main_pa)
        && tabla_ok;
    drivers::serial::write_str(if ok { "[HH] nucleo en mitad superior: rip=" } else { "[HH-FALLO] mapa de la mitad superior incoherente: rip=" });
    drivers::serial::write_hex(rip);
    drivers::serial::write_str(" rsp=");
    drivers::serial::write_hex(rsp);
    drivers::serial::write_str(" rust_main=");
    drivers::serial::write_hex(main_va);
    drivers::serial::write_str(" fisico=");
    drivers::serial::write_hex(main_pa);
    drivers::serial::write_str(" traducido=");
    drivers::serial::write_hex(traducido.unwrap_or(0));
    drivers::serial::write_str(" cr3=");
    drivers::serial::write_hex(cr3 as usize);
    drivers::serial::write_str("\n");
    if !ok {
        loop { unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)); } }
    }
}

// ── Punto de entrada ──────────────────────────────────────────────────────────

#[no_mangle]
extern "C" fn rust_main(boot_info: *const bootinfo::PortixBootInfo) -> ! {
    drivers::serial::init();
    unsafe { bootinfo::init(boot_info); }
    unsafe { init_cpu_features(); }
    // [Fase B.4] marcos de usuario desde la RAM del firmware, fuera del montículo
    let _ = mem::marcos::init();
    verificar_mitad_superior();

    // Verificar que SSE está habilitado correctamente
unsafe {
    let cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, nomem));
    drivers::serial::write_str("[CPU] CR4=");
    drivers::serial::write_hex(cr4 as usize);
    drivers::serial::write_str("\n");
    // Debe tener bits 9 y 10 (OSFXSR=0x200, OSXMMEXCPT=0x400)
    if cr4 & 0x600 != 0x600 {
        drivers::serial::write_str("[CPU] WARN: SSE no habilitado correctamente\n");
    }
}

    // IDT primero — sin esto cualquier excepción = triple fault opaco
    unsafe { arch::idt::init_idt(); }  // instala GDT+TSS+IDT y hace STI
    // [T-025 E3a] GS por CPU del BSP: lo necesitan syscall_entry y las ISR.
    arch::percpu::instalar(0, (core::arch::x86_64::__cpuid(1).ebx >> 24) & 0xFF);

    unsafe { ALLOCATOR.init(); }
    mem::paging::init();
    process::init();
    crate::ipc::init();
    crate::lkm::init();
    time::pit::init();
    time::rtc::init();
    drivers::serial::log("PIT", "temporizador 100 Hz");

    let hw = arch::hardware::HardwareInfo::detect_all();
    drivers::serial::log("HW", hw.cpu.brand_str());

    let pci = drivers::bus::pci::PciBus::scan();
    {
        let mut t = [0u8; 16];
        let s = util::fmt::fmt_u32(pci.count as u32, &mut t);
        drivers::serial::write_str("PCI: ");
        drivers::serial::write_str(s);
        drivers::serial::write_str(" dispositivos\n");
    }

    crate::drivers::audio::audio_init();
    crate::drivers::storage::ahci::ahci_init();
    crate::drivers::storage::nvme::nvme_init();
    crate::drivers::bus::xhci::xhci_init();
    // ENSAYO DE SILICIO: comprueba sobre el hardware real (identidad leída del
    // controlador, firmas escritas por el anfitrión y ensayos de ida y vuelta)
    // que los cuatro controladores operan de verdad.
    let silicio = crate::drivers::silicon::ejecutar_ensayo();
    crate::drivers::silicon::registrar_resultado(silicio);
    crate::arch::smp::init();
    crate::arch::aarch64::aarch64_hal_init();
    crate::drivers::storage::cow::cow_init();
    crate::drivers::storage::luks::init();
    crate::drivers::storage::nix_store::init();
    crate::ui::font_ttf::init();
    crate::ui::widgets::init();
    crate::ui::wayland_server::init();
    crate::drivers::bus::virtio_gpu::virtio_gpu_init();
    crate::arch::vmx::vmx_init();
    crate::arch::microvm::init();
    crate::arch::cfs::init();
    crate::graphics::vulkan_icd::vulkan_init();
    crate::ui::wayland_effects::wayland_effects_init();
    crate::arch::microkernel::microkernel_init();
    crate::ui::tiling_wm::tiling_wm_init();
    crate::ui::wayland_seat::wayland_seat_init();
    crate::arch::io_uring::io_uring_init();
    crate::arch::power::power_init();
    crate::drivers::storage::installer::installer_init();
    crate::arch::container::container_init();
    crate::arch::acpi_sleep::acpi_sleep_init();
    crate::arch::vmx_hypervisor::vmx_hypervisor_init();
    crate::arch::backlight::backlight_init();
    crate::arch::post_quantum::post_quantum_init();
    crate::arch::riscv64::riscv64_init();
    crate::microkernel::ebpf::ebpf_init();
    crate::drivers::audio::rt_audio::rt_audio_init();
    crate::wasm::component_model::wasi_components_init();
    crate::drivers::storage::portix_dfs::portix_dfs_init();
    crate::ui::web_engine::web_engine_init();
    crate::net::wifi_80211::wifi_init();
    crate::mem::zswap::zswap_init();
    crate::arch::sovereign_boot::sovereign_boot_init();
    crate::arch::tpm_secure::tpm_secure_init();
    crate::net::smb_thunderbolt::smb_thunderbolt_init();
    crate::arch::deployment_profile::deployment_profile_init();
    crate::system::sysinfo::system_release_init();

    // Paquetes 1, B y C: Silicio HugePages, GPU DRM/KMS y Portix-UI Desktop Shell
    let _ = crate::mem::hugepages::allocate_huge_page_2mb(0x2000_0000, 1, false);
    crate::drivers::gpu::init_gpu_subsystem();
    crate::ui::portix_ui::init_portix_ui();
    crate::ui::web_accel::init_web_accel();
    crate::arch::laptop_power::init_laptop_power();
    crate::containers::oci_runtime::init_oci_runtime();
    crate::drivers::storage::ntfs_exfat::init_peripherals();
    crate::drivers::storage::immutable_cow::init_immutable_cow();
    crate::security::tpm2_pcr::init_tpm_security();
    crate::security::packet_filter::init_packet_filter();
    crate::dev::toolchain::init_dev_toolchains();
    crate::ui::commercial_oobe::init_commercial_oobe();
    crate::net::wifi_portal::init_wifi_portal();
    crate::drivers::cpu::msr_telemetry::init_cpu_msr_telemetry();
    crate::system::subsystem_suite::init_subsystems_suite();
    drivers::serial::log("MICROARCH", "HugePages & Per-CPU Slabs Initialized");
    drivers::serial::log("GPU", "DRM/KMS Native GPU Subsystem Initialized");
    drivers::serial::log("PORTIX-UI", "Reactive Shell & Gestures Initialized");
    drivers::serial::log("WEB-ACCEL", "DRM/KMS Accelerated DOM & VA-API Ready");
    drivers::serial::log("LAPTOP-PWR", "Modern Standby S0ix & Fn-Keys Ready");
    drivers::serial::log("PORTIX-BOX", "Lightweight OCI & Flatpak Runtime Ready");
    drivers::serial::log("GAMING-HDR", "Multimonitor 4K@144Hz, HDR10 & DualSense Ready");
    drivers::serial::log("PERIPHERALS", "NTFS/exFAT R/W & TB4 Docks Ready");
    drivers::serial::log("IMMUTABLE", "A/B Slots & Instant CoW Snapshots Ready");
    drivers::serial::log("TPM-PRIVACY", "TPM 2.0 PCR0-7 Disk Seal & Zero-Telemetry Ready");
    drivers::serial::log("DEV-TOOL", "Rust/Py/Go/C++ Toolchains, LSP Server & GPU Splits Ready");
    drivers::serial::log("OOBE-STORE", "OOBE Wizard, Dual-Boot Win11 & Portix-Store Ready");
    drivers::serial::log("WIFI-PORTAL", "Wi-Fi 6E/7 Tri-Band & xdg-desktop-portal Ready");
    drivers::serial::log("TITAN-v2", "Portix OS v2.0 'Titan', 452 Syscalls & RAPL Silicon Ready");
    drivers::serial::log("EPICS-20-25", "Epicas 20 a 25 Consolidadas (Linux ABI, Web, Flatpak, Laptops, GPU, Perif)");

    let mut kbd = drivers::input::keyboard::KeyboardState::new();
    let mut ms = drivers::input::mouse::MouseState::new();
    let mut c = Console::new();
    let lay = Layout::new(c.width(), c.height());
    ms.init(lay.fw.max(1), lay.fh.max(1));
    {
        let mut gm = drivers::input::mouse::GLOBAL_MOUSE.lock();
        gm.max_x = ms.max_x;
        gm.max_y = ms.max_y;
        gm.x = ms.x;
        gm.y = ms.y;
        gm.present = ms.present;
    }

    // ── Drenar FIFO PS/2 ──────────────────────────────────────────────
    // Los bytes basura en el FIFO del 8042 (p.ej. BAT completion del
    // teclado durante el boot) bloquean la generacion de nuevas IRQs: el
    // 8042 solo genera IRQ1/IRQ12 en la transicion vacio→no-vacio.
    // Si el FIFO contiene bytes viejos, las IRQs nunca se disparan.
    unsafe {
        let mut drained = 0u32;
        for _ in 0..256 {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x01 == 0 { break; }
            let _byte: u8;
            core::arch::asm!("in al, dx", out("al") _byte, in("dx") 0x60u16, options(nostack));
            drained += 1;
        }
        if drained > 0 {
            crate::drivers::serial::write_str("[PS2] drained ");
            crate::drivers::serial::write_usize(drained as usize);
            crate::drivers::serial::write_str(" stale bytes\n");
        }

        // Segunda fase: habilitar reloj del teclado (bit4=0) + traduccion (bit6=1)
        // La primera fase en mouse.rs deshabilito el reloj para evitar interferencia.
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0x60u8, options(nostack));
        core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        // 0x43 = IRQ1+IRQ12+translation ON, keyboard+mouse clock enabled
        core::arch::asm!("out dx, al", in("dx") 0x60u16, in("al") 0x43u8, options(nostack));
        crate::drivers::serial::log("PS2", "config 0x43 (kbd clock + translation ON)");

        // Habilitar scanning del teclado (0xF4)
        // El ACK (0xFA) llega via IRQ1 → SCANCODE_BUF (se ignora)
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        core::arch::asm!("out dx, al", in("dx") 0x60u16, in("al") 0xF4u8, options(nostack, nomem));
        crate::drivers::serial::log("KBD", "enable cmd sent (0xF4)");
    }

    let mut term = console::terminal::Terminal::new();
    term.write_line("PORTIX v1.0.0 Genesis  Kernel Bare-Metal (Official Release)", LineColor::Header);
    term.write_line("Escribe 'ayuda' para comandos.", LineColor::Info);
    term.write_empty();

    crate::drivers::storage::vfs::mount("/tmp", crate::drivers::storage::vfs::FsType::RamFs);
    crate::drivers::storage::vfs::mount("/dev/shm", crate::drivers::storage::vfs::FsType::RamFs);

    let mut mount_ok = false;


    {
        let bus = ata::AtaBus::scan();
        ata::log_drives(&bus);

        // Cachear info del Primary0 para comandos sin re-escanear hardware
        if let Some(info) = bus.info(ata::DriveId::Primary0) {
            ata::store_primary_drive_info(*info);
        }

        // Registrar todos los drives en el DeviceRegistry
        for info in bus.iter() {
            let dev: Box<dyn BlockDevice> = if info.kind == DriveType::Atapi {
                Box::new(atapi::AtapiDrive::new(*info))
            } else {
                Box::new(ata::AtaDrive::from_info(*info))
            };
            registry::register_device(dev);
        }

        // Montar FAT32 o formatear si no existe (usa device 0 = Primary0)
        let dev0_kind = registry::with_device(0, |d| d.map(|d| d.device_info().kind));

        // Si device 0 es ATAPI (CD-ROM), mostrar guía en vez de montar/formatear
        if let Some(DriveType::Atapi) = dev0_kind {
            serial::log_level(serial::Level::Info, "BOOT",
                "Boot desde CD-ROM detectado. Ejecuta 'install' para instalar PORTIX en disco duro.");
        }

        serial::write_str("[FS] dev0_kind=");
        serial::write_usize(match dev0_kind { Some(DriveType::Ata) => 1, Some(DriveType::Atapi) => 2, None => 0 });
        serial::write_str("\n");
        if let Some(DriveType::Ata) = dev0_kind {
            serial::write_str("[FS] Attempting FAT32 mount...\n");
            registry::with_device(0, |d| {
                if let Some(drive) = d {
                    serial::write_str("[FS] Got drive, mounting...\n");
                    let mut mbr = [0u8; 512];
                    if drive.read_sectors(0, 1, &mut mbr).is_ok() {
                        serial::write_str("[FS] MBR read OK, sig=");
                        serial::write_hex(mbr[510] as usize);
                        serial::write_hex(mbr[511] as usize);
                        serial::write_str("\n");
                        let is_gpt = crate::drivers::storage::gpt::is_gpt_disk(&mbr);
                        if is_gpt {
                            serial::write_str("[FS] GPT protective MBR detectado\n");
                        } else {
                            serial::write_str("[FS] P1 type=");
                            serial::write_hex(mbr[0x1BE+4] as usize);
                            serial::write_str(" P2 type=");
                            serial::write_hex(mbr[0x1CE+4] as usize);
                            serial::write_str("\n");
                        }
                    }
                    if let Ok(_) = fat32::Fat32Volume::mount(drive) {
                        mount_ok = true;
                        serial::log("FS", "FAT32 montado correctamente");
                    } else {
                        serial::log("FS", "FAT32 no encontrado en particion");
                    }
                }
            });
            serial::write_str("[FS] mount_ok=");
            serial::write_usize(mount_ok as usize);
            serial::write_str("\n");
        }
        // [CICLO 51 / LOTE 2] Partición ext2 real (MBR 0x83 o GPT "Linux
        // filesystem") montada en /usr/local bajo el contrato SistemaDeFicheros.
        if let Some(DriveType::Ata) = dev0_kind {
            match crate::fs::ext2::hallar_particion() {
                Some((lba, n)) => match crate::fs::ext2::Ext2::montar(lba, n) {
                    Ok(fs) => {
                        let (bs, bt, bl, it, il) = fs.resumen();
                        let caja: alloc::boxed::Box<dyn crate::fs::SistemaDeFicheros> = alloc::boxed::Box::new(fs);
                        // [L3] La particion ext2 ES la raiz: todo lo que no tenga un
                        // montaje mas especifico (/tmp, /dev/shm, /dev, /proc) se sirve
                        // desde aqui. La FAT queda como particion de arranque/compatibilidad.
                        if crate::drivers::storage::vfs::montar_generico("/", crate::drivers::storage::vfs::FsType::Ext2, caja) {
                            serial::write_str("[EXT2] montado en / (raiz): lba=");
                            serial::write_usize(lba as usize);
                            serial::write_str(" bloque=");
                            serial::write_usize(bs as usize);
                            serial::write_str(" bloques=");
                            serial::write_usize(bt as usize);
                            serial::write_str(" libres=");
                            serial::write_usize(bl as usize);
                            serial::write_str(" inodos=");
                            serial::write_usize(it as usize);
                            serial::write_str(" libres=");
                            serial::write_usize(il as usize);
                            serial::write_str("\n");
                            // [L3c] La FAT de arranque (cargador + nucleo) se monta en
                            // /boot de SOLO LECTURA bajo el contrato; el resto del
                            // arbol vive en la raiz ext2.
                            if mount_ok {
                                let fat = alloc::boxed::Box::new(crate::fs::fat_adaptador::FatSoloLectura::nuevo(0));
                                if crate::drivers::storage::vfs::montar_generico("/boot", crate::drivers::storage::vfs::FsType::Fat32, fat) {
                                    serial::log("FS", "FAT de arranque montada en /boot (solo lectura)");
                                }
                            }
                        } else {
                            serial::log("EXT2", "sin hueco en la tabla de montajes");
                        }
                    }
                    Err(m) => { serial::write_str("[EXT2] no montado: "); serial::write_str(m); serial::write_str("\n"); }
                },
                None => serial::log("EXT2", "sin particion ext2 en el disco 0"),
            }
        }
        serial::write_str("[FS] After mount block, mount_ok=");
        serial::write_usize(mount_ok as usize);
        serial::write_str("\n");

        // [T-001] Antes, si la FAT no montaba, el nucleo FORMATEABA el disco 0
        // (`mkfs::auto_format`) y listaba /bin /home /etc por el serie. Un
        // nucleo no destruye datos para arrancar: eliminado. Sin FAT no hay
        // /boot; sin ext2 no hay raiz (modo degradado explicito mas abajo).
        crate::net::init();
    }

    let mut win_mgr = ui::WindowManager::new();
    let mut last_blink_tick = 0u64;
    let mut last_render_tick = 0u64;
    let mut needs_draw = true;
    let mut needs_present = true;

    c.clear(Color::PORTIX_BG);

    // Presentar Escritorio Real Moderno (elimina interfaz estatica por pestañas)
    win_mgr.draw(&mut c, &lay, &hw);
    drivers::serial::log("FB", "presenting Real Desktop UI...");
    c.present_full();
    drivers::serial::log("FB", "present done");
    crate::system::bench::marcar_escritorio_listo(); // [T-011] instante medible de "escritorio listo"

    // ── Ring 3 — init process (scheduler-managed) ────────────────────────
    // Create PID 1 in Ready state. Process is NOT auto-executed — it enters
    // the scheduler's ready pool for preemptive multitasking (future).
    // The kernel boots directly into the main loop UI.
    let mut r3_loaded = false;
    // [CICLO 51 / L3] PID 1 se carga desde la RAIZ ext2 por el contrato;
    // la FAT queda solo como rescate si la raiz no se monto.
    if crate::drivers::storage::vfs::es_ruta_fs("/bin/init") {
        for name in ["/bin/init", "/bin/sh"] {
            if let Some(buf) = elf::read_binary_file(name) {
                serial::write_str("[FS] "); serial::write_str(name);
                serial::write_str(" cargado desde la raiz ext2 ("); serial::write_usize(buf.len()); serial::write_str(" bytes)\n");
                if let Some(pid1) = elf::elf_load_and_create_process(&buf, &name[5..]) {
                    if let Some(proc) = process::process_by_pid(pid1) { proc.state = process::ProcessState::Blocked; }
                    let stack_top = core::ptr::addr_of!(__stack_top) as u64;
                    process::set_tss_rsp0(stack_top);
                    serial::write_str("[R3] "); serial::write_str(&name[5..]); serial::write_str(" process PID=1 registered\n");
                    r3_loaded = true;
                    break;
                }
            }
        }
    }
    if !r3_loaded {
        // [T-001] MODO DEGRADADO explicito: no hay raiz ext2 (o no contiene
        // /bin/init ni /bin/sh). Antes se intentaba la FAT y, si no, un
        // `hello.elf` embebido que fingia un PID 1. Ahora: sin PID 1, consola
        // del nucleo solamente, y un mensaje inequivoco por el serie y la
        // pantalla. La suite comprueba este camino arrancando sin P3.
        crate::drivers::storage::vfs::marcar_modo_degradado();
        serial::write_str("[MODO DEGRADADO] raiz ext2 no montada o sin /bin/init: sin PID 1; consola del nucleo solamente\n");
        term.write_line("MODO DEGRADADO: raiz ext2 no montada o sin /bin/init.", console::terminal::LineColor::Error);
        term.write_line("No hay PID 1 ni programas de usuario; solo la consola del nucleo.", console::terminal::LineColor::Error);
        needs_draw = true;
    }

    loop {
        // [T-026, defecto 23] La consola vuelve aquí tras cada proceso por la
        // continuación `ring3_ret_addr`, que llega desde el camino de salida de
        // una syscall (IF=0 por FMASK): el bucle quedaba sordo al PIT y a la red
        // hasta el siguiente `run` (hueco medido de ~900 ms entre dos `run`).
        // La consola ociosa siempre debe tener las interrupciones abiertas.
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        let now = time::pit::ticks();

        // ── Poll Network (e1000 / ARP / ICMP / IPv4) ─────────────────────
        crate::net::poll();

        // ── Drenado IRQ1 teclado ─────────────────────────────────────────
        // IRQ1 handler buffers scancodes into SCANCODE_BUF.
        // IRQ12 (mouse) buffers mouse bytes into MOUSE_BUF.
        let mut kbd_buf = [0u8; 32];
        let mut kbd_n = 0usize;
        let mut ms_buf = [0u8; 32];
        let mut ms_n = 0usize;
        // Drain IRQ1 scancode buffer
        while kbd_n < 32 {
            match crate::arch::isr_handlers::pop_scancode() {
                Some(sc) => { kbd_buf[kbd_n] = sc; kbd_n += 1; }
                None => break,
            }
        }
        // Drain IRQ12 mouse buffer
        while ms_n < 32 {
            match crate::arch::isr_handlers::pop_mouse_byte() {
                Some(byte) => { ms_buf[ms_n] = byte; ms_n += 1; }
                None => break,
            }
        }

        // ── Poll mouse directly (fallback if IRQ12 doesn't fire) ──────────
        while ms_n < 32 {
            match unsafe { drivers::input::mouse::poll_aux() } {
                Some(byte) => {
                    ms_buf[ms_n] = byte; ms_n += 1;
                }
                None => break,
            }
        }

        // ── Drenado de entrada serie COM1 (Terminal interactiva / Headless) ──
        while let Some(b) = process::tty_read_byte() {
            needs_draw = true;
            match b {
                b'\r' | b'\n' => {
                    drivers::serial::write_str("\r\n");
                    term.enter(&hw, &pci);
                }
                0x08 | 0x7F => {
                    term.backspace();
                    drivers::serial::write_str("\x08 \x08");
                }
                32..=126 => {
                    drivers::serial::write_byte(b);
                    term.type_char(b);
                }
                _ => {}
            }
        }

        // ── Cola de teclado ───────────────────────────────────────────────
        for i in 0..kbd_n {
            if let Some(key) = kbd.feed_byte(kbd_buf[i]) {
                needs_draw = true;

                // Editor de texto del terminal (modo especial)
                if let Some(ed) = term.editor.as_mut() {
                    let ctrl = kbd.ctrl();
                    ed.handle_key(key, ctrl);
                    if ed.exit {
                        term.editor = None;
                        term.write_line("  Editor cerrado.", LineColor::Info);
                    }
                    continue;
                }

                // Escape cierra el menú de inicio si está abierto
                if key == Key::Escape {
                    if win_mgr.start_menu_open {
                        win_mgr.start_menu_open = false;
                        needs_draw = true;
                    }
                    continue;
                }

                match key {
                    // Teclas de acceso rápido a ventanas del escritorio
                    Key::F1 => {
                        win_mgr.windows[0].visible = true;
                        win_mgr.bring_to_front(0);
                        needs_draw = true;
                    }
                    Key::F2 => {
                        win_mgr.windows[1].visible = true;
                        win_mgr.bring_to_front(1);
                        needs_draw = true;
                    }
                    Key::F3 => {
                        win_mgr.windows[2].visible = true;
                        win_mgr.bring_to_front(2);
                        needs_draw = true;
                    }
                    Key::F4 => {
                        win_mgr.windows[3].visible = true;
                        win_mgr.bring_to_front(3);
                        needs_draw = true;
                    }
                    Key::F5 => {
                        win_mgr.restore_window(4);
                        needs_draw = true;
                    }
                    Key::F6 => {
                        win_mgr.restore_window(5);
                        needs_draw = true;
                    }
                    Key::F7 => {
                        win_mgr.restore_window(6); // Navegador Web
                        needs_draw = true;
                    }
                    Key::F8 => {
                        win_mgr.restore_window(7); // DOOM
                        needs_draw = true;
                    }
                    Key::F9 => {
                        win_mgr.restore_window(8); // Vulkan 3D
                        needs_draw = true;
                    }
                    Key::F10 => {
                        win_mgr.restore_window(9); // Tienda P2P
                        needs_draw = true;
                    }
                    Key::F11 => {
                        win_mgr.restore_window(10); // Sintetizador Audio
                        needs_draw = true;
                    }
                    Key::F12 => {
                        win_mgr.start_menu_open = !win_mgr.start_menu_open;
                        needs_draw = true;
                    }

                    Key::Tab => {
                        win_mgr.cycle_focus();
                        needs_draw = true;
                    }

                    // Escritorio: Despacho de caracteres a la ventana enfocada
                    Key::Char(ch) => {
                        if win_mgr.handle_key_char(ch) {
                            needs_draw = true;
                        }
                    }
                    Key::Backspace => {
                        if win_mgr.handle_backspace() {
                            needs_draw = true;
                        }
                    }
                    Key::Enter => {
                        if win_mgr.handle_enter(&hw) {
                            needs_draw = true;
                        }
                    }

                    Key::Up | Key::Down | Key::Left | Key::Right | Key::Escape => {
                        if win_mgr.handle_key_special(key) {
                            needs_draw = true;
                        }
                    }

                    _ => {}
                }
            }
        }

        // ── Cola de ratón ─────────────────────────────────────────────────
        let mouse_changed = if ms.present && ms_n > 0 {
            ms.begin_frame();
            let mut changed = false;
            for i in 0..ms_n {
                if ms.feed(ms_buf[i]) {
                    changed = true;
                }
            }
            if ms.error_count >= 25 {
                ms.intelligent_reset();
            }
            {
                let mut gm = drivers::input::mouse::GLOBAL_MOUSE.lock();
                gm.x = ms.x;
                gm.y = ms.y;
                gm.buttons = ms.buttons;
                gm.prev_buttons = ms.prev_buttons;
                gm.scroll_delta = ms.scroll_delta;
                gm.present = ms.present;
            }
            changed
        } else {
            false
        };

        if mouse_changed {
            needs_draw = true;
        }

        // ── Interacción con ratón en el Escritorio Real ──────────────────
        if ms.scroll_delta != 0 && win_mgr.handle_scroll(ms.scroll_delta) {
            needs_draw = true;
        }

        if ms.left_btn() && win_mgr.handle_mouse_move(ms.x, ms.y) {
            needs_draw = true;
        }
        if ms.left_released() && win_mgr.handle_mouse_up() {
            needs_draw = true;
        }

        if mouse_changed && ms.left_clicked() {
            if win_mgr.handle_mouse_down(ms.x, ms.y, &hw) {
                needs_draw = true;
            }
        }
        if mouse_changed && ms.right_clicked() {
            if win_mgr.handle_right_click(ms.x, ms.y) {
                needs_draw = true;
            }
        }

        // ── Cursor parpadeante ────────────────────────────────────────────
        if now.wrapping_sub(last_blink_tick) >= 50 {
            last_blink_tick = now;
            needs_draw = true;
        }

        // ── Render Escritorio Real Moderno ────────────────────────────────
        if needs_draw {
            win_mgr.draw(&mut c, &lay, &hw);

            if ms.present {
                c.draw_cursor(ms.x, ms.y);
            }
            needs_draw = false;
            needs_present = true;
        }

        if needs_present && now.wrapping_sub(last_render_tick) >= RENDER_INTERVAL {
            c.present();
            last_render_tick = now;
            needs_present = false;
        }

        unsafe {
            core::arch::asm!("pause", options(nostack, nomem));
        }
    }
}
