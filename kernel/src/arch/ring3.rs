use core::arch::asm;

#[no_mangle]
pub unsafe extern "C" fn enter_ring3_setup() {
    let proc = crate::process::current_process()
        .expect("enter_ring3_setup: no current process");

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
        "iretq",
        ss = in(reg) user_ds,
        rsp3 = in(reg) proc.user_rsp,
        rflags = in(reg) 0x202u64,
        cs = in(reg) user_cs,
        rip3 = in(reg) proc.user_rip,
    );

    loop {
        core::hint::spin_loop();
    }
}

extern "C" {
    pub fn enter_ring3_asm();
}
