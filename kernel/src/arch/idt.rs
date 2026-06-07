// kernel/src/idt.rs — PORTIX v8 — GDT con USER_CODE/USER_DATA, syscall MSRs
#![allow(dead_code)]

use core::arch::asm;

// ── GDT selectors (actualizados para ring 3) ────────────────────────────
pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS:   u16 = 0x18;   // DPL=3
pub const USER_CS:   u16 = 0x20;   // DPL=3
pub const TSS_SEL:   u16 = 0x28;

// ── MSRs para syscall/sysret ────────────────────────────────────────────
const MSR_STAR:  u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_FMASK: u32 = 0xC000_0084;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low:  u16,
    selector:    u16,
    ist:         u8,
    type_attr:   u8,
    offset_mid:  u16,
    offset_high: u32,
    reserved:    u32,
}

const GATE_INT: u8 = 0x8E;
const GATE_TRAP: u8 = 0x8F;

impl IdtEntry {
    const fn new() -> Self {
        Self { offset_low:0, selector:0, ist:0,
               type_attr:0, offset_mid:0, offset_high:0, reserved:0 }
    }
    fn set(&mut self, handler: u64, ist: u8, attr: u8) {
        self.offset_low  = (handler & 0xFFFF) as u16;
        self.offset_mid  = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector    = KERNEL_CS;
        self.ist         = ist;
        self.type_attr   = attr;
        self.reserved    = 0;
    }
    fn set_handler(&mut self, h: u64)      { self.set(h, 0, GATE_INT); }
    fn set_handler_ist1(&mut self, h: u64) { self.set(h, 1, GATE_INT); }
}

#[repr(C, packed)] struct IdtPtr { limit: u16, base: u64 }
#[repr(C, packed)] struct GdtPtr { limit: u16, base: u64 }

#[repr(C, packed)]
pub struct Tss {
    _res0:      u32,
    pub rsp:    [u64; 3],
    _res1:      u64,
    ist:        [u64; 7],
    _res2:      u64,
    _res3:      u16,
    iomap_base: u16,
}

#[repr(align(16))]
struct Stack16K([u8; 16384]);

static mut DF_STACK: Stack16K = Stack16K([0u8; 16384]);

static mut TSS: Tss = Tss {
    _res0:0, rsp:[0;3], _res1:0, ist:[0;7], _res2:0, _res3:0,
    iomap_base: core::mem::size_of::<Tss>() as u16,
};

// ── GDT con 7 entradas: NULL | KERN_CODE | KERN_DATA | USER_DATA | USER_CODE | TSS_LOW | TSS_HIGH
#[repr(C, align(16))]
struct Gdt {
    null:      u64,
    code64:    u64,  // 0x08  KERNEL_CODE  — ring 0, 64-bit
    data64:    u64,  // 0x10  KERNEL_DATA  — ring 0
    user_data: u64,  // 0x18  USER_DATA    — ring 3
    user_code: u64,  // 0x20  USER_CODE    — ring 3
    tss_low:   u64,  // 0x28
    tss_high:  u64,  // 0x30
}

static mut GDT: Gdt = Gdt {
    null:   0x0000_0000_0000_0000,
    code64: 0x00AF_9A00_0000_FFFF,
    data64: 0x00CF_9200_0000_FFFF,
    // DPL=3: access byte = 0xF2 (data, writable) / 0xFA (code, readable)
    user_data: 0x00CF_F200_0000_FFFF,
    user_code: 0x00AF_FA00_0000_FFFF,
    tss_low: 0, tss_high: 0,
};

static mut GDT_PTR: GdtPtr = GdtPtr { limit:0, base:0 };
static mut IDT_PTR: IdtPtr = IdtPtr { limit:0, base:0 };

#[no_mangle]
static mut IDT: [IdtEntry; 256] = [IdtEntry::new(); 256];

extern "C" {
    fn isr_0();  fn isr_1();  fn isr_2();  fn isr_3();
    fn isr_4();  fn isr_5();  fn isr_6();  fn isr_7();
    fn isr_8();
    fn isr_10(); fn isr_11(); fn isr_12();
    fn isr_13(); fn isr_14();
    fn isr_16(); fn isr_17(); fn isr_18(); fn isr_19();
    pub fn reload_segments();
    fn irq0_handler();
    fn irq_stub_master();
    fn irq_stub_slave();
    fn syscall_entry();
    fn int80_handler();
}

pub fn get_tss_ptr() -> *mut Tss {
    core::ptr::addr_of_mut!(TSS)
}

pub unsafe fn init_idt() {
    let kernel_stack_top = core::ptr::addr_of!(crate::__stack_top) as u64;

    // 1. IST1 for #DF + RSP0 for ring 3 → ring 0
    let df_top = (core::ptr::addr_of!(DF_STACK) as *const u8)
        .add(core::mem::size_of::<Stack16K>()) as u64;
    TSS.ist[0] = df_top;
    TSS.rsp[0] = kernel_stack_top;

    // 2. Build TSS descriptor (selector 0x28 = TSS_SEL)
    let base  = core::ptr::addr_of!(TSS) as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    GDT.tss_low =
          (limit  & 0x0000_FFFF)
        | ((base  & 0x00FF_FFFF) << 16)
        | 0x0000_8900_0000_0000_u64
        | ((limit & 0x000F_0000) << 32)
        | ((base  & 0xFF00_0000) << 32);
    GDT.tss_high = (base >> 32) & 0xFFFF_FFFF;

    // 3. Load GDT (7 entradas × 8 = 56 bytes, limit = 55)
    GDT_PTR.limit = (core::mem::size_of::<Gdt>() - 1) as u16;
    GDT_PTR.base  = core::ptr::addr_of!(GDT) as u64;
    asm!("lgdt [{p}]", p = in(reg) core::ptr::addr_of!(GDT_PTR),
         options(nostack, preserves_flags, readonly));

    // 4. Reload CS (far return: 0x08 → CS)
    reload_segments();

    // 5. Data selectors
    asm!(
        "mov ax, 0x10", "mov ds, ax", "mov es, ax", "mov ss, ax",
        "xor ax, ax",   "mov fs, ax", "mov gs, ax",
        out("ax") _, options(nostack, preserves_flags)
    );

    // 6. Load TSS (selector 0x28)
    asm!("ltr ax", in("ax") TSS_SEL, options(nostack, preserves_flags));

    // 7. CPU exception handlers
    macro_rules! h { ($f:expr) => { core::mem::transmute::<unsafe extern "C" fn(), u64>($f) } }
    IDT[ 0].set_handler(h!(isr_0));
    IDT[ 1].set_handler(h!(isr_1));
    IDT[ 2].set_handler(h!(isr_2));
    IDT[ 3].set_handler(h!(isr_3));
    IDT[ 4].set_handler(h!(isr_4));
    IDT[ 5].set_handler(h!(isr_5));
    IDT[ 6].set_handler(h!(isr_6));
    IDT[ 7].set_handler(h!(isr_7));
    IDT[ 8].set_handler_ist1(h!(isr_8));   // #DF on IST1
    IDT[10].set_handler(h!(isr_10));
    IDT[11].set_handler(h!(isr_11));
    IDT[12].set_handler(h!(isr_12));
    IDT[13].set_handler(h!(isr_13));
    IDT[14].set_handler(h!(isr_14));
    IDT[16].set_handler(h!(isr_16));
    IDT[17].set_handler(h!(isr_17));
    IDT[18].set_handler(h!(isr_18));
    IDT[19].set_handler(h!(isr_19));

    // 8. IRQ handlers — IRQ0 (PIT) gets its own handler
    let irq0  = core::mem::transmute::<unsafe extern "C" fn(), u64>(irq0_handler);
    let irq_m = core::mem::transmute::<unsafe extern "C" fn(), u64>(irq_stub_master);
    let irq_s = core::mem::transmute::<unsafe extern "C" fn(), u64>(irq_stub_slave);
    IDT[0x20].set_handler(irq0);
    for i in 0x21..=0x27_usize { IDT[i].set_handler(irq_m); }
    for i in 0x28..=0x2F_usize { IDT[i].set_handler(irq_s); }

    // 9. IDT[0x80] — int 0x80 syscall gate (DPL=3, trap gate so IF stays on)
    IDT[0x80].set(h!(int80_handler), 0, 0xEF);

    // 11. Load IDTR
    IDT_PTR.limit = (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16;
    IDT_PTR.base  = core::ptr::addr_of!(IDT) as u64;
    asm!("lidt [{p}]", p = in(reg) core::ptr::addr_of!(IDT_PTR),
         options(nostack, preserves_flags, readonly));

    // 12. Remap PIC + unmask IRQ0 only
    core::arch::asm!("out 0x20, al", in("al") 0x11u8, options(nostack, nomem));
    core::arch::asm!("out 0xA0, al", in("al") 0x11u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0x21, al", in("al") 0x20u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0xA1, al", in("al") 0x28u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0x21, al", in("al") 0x04u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0xA1, al", in("al") 0x02u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0x21, al", in("al") 0x01u8, options(nostack, nomem));
    core::arch::asm!("out 0xA1, al", in("al") 0x01u8, options(nostack, nomem));
    core::arch::asm!("out 0x80, al", in("al") 0x00u8, options(nostack, nomem));
    core::arch::asm!("out 0x21, al", in("al") 0xFEu8, options(nostack, nomem));
    core::arch::asm!("out 0xA1, al", in("al") 0xFFu8, options(nostack, nomem));

    // 13. MSR setup for syscall/sysret
    // STAR[47:32] = KERNEL_DS(0x10) → SYSCALL CS, SYSRET CS base
    // STAR[63:48] = KERNEL_DS(0x10) → SYSCALL SS base, SYSRET SS base
    // SYSCALL: CS ignored (fake), SS = 0x10+8 = 0x18 (USER_DATA, fake)
    // SYSRET:  CS = (0x10+16)|3 = 0x23 → USER_CODE(0x20) ✓
    //          SS = (0x10+8)|3  = 0x1B → USER_DATA(0x18)  ✓
    let star = (KERNEL_CS as u64) << 16 | (KERNEL_DS as u64) << 32
             | (KERNEL_DS as u64) << 48;
    // DEBUG: print star before wrmsr
    crate::drivers::serial::write_str("star=");
    let hex = b"0123456789ABCDEF";
    let hi = (star >> 32) as u32;
    let lo = star as u32;
    for sh in (0..8).rev() { crate::drivers::serial::write_byte(hex[((hi >> (sh*4)) & 0xF) as usize]); }
    crate::drivers::serial::write_byte(b':');
    for sl in (0..8).rev() { crate::drivers::serial::write_byte(hex[((lo >> (sl*4)) & 0xF) as usize]); }
    crate::drivers::serial::write_byte(b'\n');
    let lstar = core::mem::transmute::<unsafe extern "C" fn(), u64>(syscall_entry);
    let fmask = 0x43200u64; // clear IF(9) + IOPL(12-13) + AC(18) during syscall

    // Use explicit in constraints so wrmsr sees correct values
    let star_lo = star as u32;
    let star_hi = (star >> 32) as u32;
    asm!("wrmsr",
         in("ecx") MSR_STAR, in("eax") star_lo, in("edx") star_hi,
         options(nostack, preserves_flags));
    asm!("wrmsr",
         inout("ecx") MSR_LSTAR => _, inout("eax") lstar as u32 => _, inout("edx") (lstar >> 32) as u32 => _,
         options(nostack, preserves_flags));
    asm!("wrmsr",
         inout("ecx") MSR_FMASK => _, inout("eax") fmask as u32 => _, inout("edx") (fmask >> 32) as u32 => _,
         options(nostack, preserves_flags));

    // 14. Enable interrupts
    asm!("sti", options(nostack, preserves_flags));
}