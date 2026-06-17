// kernel/src/acpi.rs — PORTIX ACPI Basic (poweroff / reboot)
#![allow(dead_code)]

#[inline(always)]
unsafe fn outw(p: u16, v: u16) {
    core::arch::asm!("out dx, ax", in("dx") p, in("ax") v, options(nostack, nomem));
}
#[inline(always)]
unsafe fn outb(p: u16, v: u8) {
    core::arch::asm!("out dx, al", in("dx") p, in("al") v, options(nostack, nomem));
}
#[inline(always)]
unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack, nomem));
    v
}

#[inline(always)]
unsafe fn io_wait() {
    outb(0x80, 0);
}

fn sync_disks() {
    crate::drivers::storage::registry::flush_all();
    unsafe {
        for _ in 0..16 {
            io_wait();
        }
    }
}

/// Power off the machine.
/// Tries VirtualBox → QEMU → Bochs in sequence.
pub fn poweroff() -> ! {
    sync_disks();
    unsafe {
        // NOTA: QEMU-only fallback. En hardware real leer FADT para PM1a_CNT_BLK.
        outw(0x4004, 0x3400); // VirtualBox (QEMU-only)
        for _ in 0..16 { io_wait(); }
        outw(0x604,  0x2000); // QEMU ≥ 2.x  ACPI PM1a (QEMU-only)
        for _ in 0..16 { io_wait(); }
        outw(0xB004, 0x2000); // Bochs / old QEMU (QEMU-only)
        for _ in 0..16 { io_wait(); }
        // Last resort: triple-fault via null IDT
        core::arch::asm!(
            "cli",
            "lidt [rip + 2f]",
            "int 3",
            "2:",
            ".word 0",         // IDT limit = 0
            ".quad 0",         // IDT base  = 0
            options(nostack, nomem)
        );
        loop { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}

/// Reboot via keyboard controller pulse.
pub fn reboot() -> ! {
    sync_disks();
    unsafe {
        core::arch::asm!("cli", options(nostack, nomem));
        // Drain the KBC input buffer
        let mut limit = 100_000u32;
        while inb(0x64) & 0x02 != 0 && limit > 0 { limit -= 1; io_wait(); }
        for _ in 0..16 { io_wait(); }
        outb(0x64, 0xFE); // Pulse CPU reset line
        for _ in 0..16 { io_wait(); }
        // Fallback: QEMU ISA reset
        outb(0x92, 0x01);
        loop { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}
