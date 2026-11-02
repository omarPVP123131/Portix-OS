use core::arch::asm;

#[no_mangle]
pub unsafe extern "C" fn enter_ring3_setup() {
    // Sin proceso actual no hay nada que ejecutar en Ring 3: se registra y se detiene
    // la CPU en lugar de entrar en panico (zero unwrap/expect en el nucleo).
    let Some(proc) = crate::process::current_process() else {
        crate::drivers::serial::write_str("[R3-FALLO] enter_ring3_setup sin proceso actual\n");
        loop { core::arch::asm!("cli; hlt"); }
    };

    crate::drivers::serial::write_str("[R3] enter PID=");
    crate::drivers::serial::write_usize(proc.pid as usize);
    crate::drivers::serial::write_str(" name='");
    crate::drivers::serial::write_str(proc.name_str());
    crate::drivers::serial::write_str("' entry=");
    crate::drivers::serial::write_hex(proc.user_rip as usize);
    crate::drivers::serial::write_str("\n");

    // Verify entry page translation BEFORE switching CR3
    let entry_page = crate::mem::paging::page_align_down(proc.user_rip as usize);
    if crate::mem::paging::translate(proc.cr3, entry_page).is_none() {
        crate::drivers::serial::write_str("[R3] CRITICAL - entry page NOT mapped!\n");
    }

    crate::mem::paging::write_cr3(proc.cr3);

    let user_cs = (crate::arch::idt::USER_CS as u64) | 3;
    let user_ds = (crate::arch::idt::USER_DS as u64) | 3;

    asm!(
        "push {ss}",
        "push {rsp3}",
        "push {rflags}",
        "push {cs}",
        "push {rip3}",
        // [T-004] ABI x86-64 de entrada de proceso: todos los registros a cero.
        // En especial RDX, que glibc interpreta como `rtld_fini` (puntero a
        // función para atexit): aquí quedaba 0x1B (USER_DS) y el estático de
        // glibc saltaba a 0x1B al terminar. musl lo ignora y por eso no se vio.
        "xor eax, eax", "xor ebx, ebx", "xor ecx, ecx", "xor edx, edx",
        "xor esi, esi", "xor edi, edi", "xor ebp, ebp",
        "xor r8d, r8d", "xor r9d, r9d", "xor r10d, r10d", "xor r11d, r11d",
        "xor r12d, r12d", "xor r13d, r13d", "xor r14d, r14d", "xor r15d, r15d",
        // [T-025 E3a] salida a Ring 3: GS del usuario; el del nucleo queda en
        // KERNEL_GS_BASE. `cli` primero: una IRQ entre `swapgs` e `iretq` veria
        // CS=0x08 y correria el nucleo con el GS del usuario (iretq restaura IF).
        "cli",
        "swapgs",
        "iretq",
        ss = in(reg) user_ds,
        rsp3 = in(reg) proc.user_rsp,
        rflags = in(reg) 0x202u64,
        cs = in(reg) user_cs,
        rip3 = in(reg) proc.user_rip,
        options(noreturn)
    );
}

extern "C" {
    pub fn enter_ring3_asm();
}
