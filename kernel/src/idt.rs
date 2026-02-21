// kernel/src/idt.rs — PORTIX v6 — IRQ0 wired to PIT, mouse IRQ12 optional
#![allow(dead_code)]

use core::arch::asm;

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

impl IdtEntry {
    const fn new() -> Self {
        Self { offset_low:0, selector:0, ist:0,
               type_attr:0, offset_mid:0, offset_high:0, reserved:0 }
    }
    fn set(&mut self, handler: u64, ist: u8, attr: u8) {
        self.offset_low  = (handler & 0xFFFF) as u16;
        self.offset_mid  = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector    = 0x08;
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
struct Tss {
    _res0:      u32,
    rsp:        [u64; 3],
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

#[repr(C, align(16))]
struct Gdt { null: u64, code64: u64, data64: u64, tss_low: u64, tss_high: u64 }

static mut GDT: Gdt = Gdt {
    null:   0x0000_0000_0000_0000,
    code64: 0x00AF_9A00_0000_FFFF,
    data64: 0x00CF_9200_0000_FFFF,
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
    fn irq0_handler();      // PIT tick — dedicated, calls pit_tick()
    fn irq_stub_master();   // IRQ 0x21-0x27 generic
    fn irq_stub_slave();    // IRQ 0x28-0x2F generic
}

pub unsafe fn init_idt() {
    // 1. IST1 for #DF
    let df_top = (core::ptr::addr_of!(DF_STACK) as *const u8)
        .add(core::mem::size_of::<Stack16K>()) as u64;
    TSS.ist[0] = df_top;

    // 2. Build TSS descriptor
    let base  = core::ptr::addr_of!(TSS) as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    GDT.tss_low =
          (limit  & 0x0000_FFFF)
        | ((base  & 0x00FF_FFFF) << 16)
        | 0x0000_8900_0000_0000_u64
        | ((limit & 0x000F_0000) << 32)
        | ((base  & 0xFF00_0000) << 32);
    GDT.tss_high = (base >> 32) & 0xFFFF_FFFF;

    // 3. Load GDT
    GDT_PTR.limit = (core::mem::size_of::<Gdt>() - 1) as u16;
    GDT_PTR.base  = core::ptr::addr_of!(GDT) as u64;
    asm!("lgdt [{p}]", p = in(reg) core::ptr::addr_of!(GDT_PTR),
         options(nostack, preserves_flags, readonly));

    // 4. Reload CS
    reload_segments();

    // 5. Data selectors
    asm!(
        "mov ax, 0x10", "mov ds, ax", "mov es, ax", "mov ss, ax",
        "xor ax, ax",   "mov fs, ax", "mov gs, ax",
        out("ax") _, options(nostack, preserves_flags)
    );

    // 6. Load TSS
    asm!("ltr ax", in("ax") 0x18_u16, options(nostack, preserves_flags));

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
    IDT[ 8].set_handler_ist1(h!(isr_8));   // #DF on dedicated stack
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

    // 9. Load IDTR
    IDT_PTR.limit = (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16;
    IDT_PTR.base  = core::ptr::addr_of!(IDT) as u64;
    asm!("lidt [{p}]", p = in(reg) core::ptr::addr_of!(IDT_PTR),
         options(nostack, preserves_flags, readonly));

    // 10. Unmask IRQ0 (PIT) only; leave all others masked
    // Master PIC mask: bit0=0 (IRQ0 unmasked), rest masked
    core::arch::asm!("out 0x21, al", in("al") 0xFEu8, options(nostack, nomem));
    core::arch::asm!("out 0xA1, al", in("al") 0xFFu8, options(nostack, nomem));

    // 11. Enable interrupts
    asm!("sti", options(nostack, preserves_flags));
}