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
// [CICLO v6.x] Puertas invocables desde Ring 3 (DPL=3): el depurador de Ring 3
// ejecuta `INT3` y `INTO` en modo usuario, y el hardware sólo admite esa
// invocación si la puerta de la IDT lo permite. Con DPL=0 el CPU eleva #GP
// con el selector del vector en lugar de entregar la interrupción.
const GATE_INT_R3: u8 = 0xEE;
const GATE_TRAP_R3: u8 = 0xEF;

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
    /// Puerta accesible desde Ring 3 (INT3 del depurador, INTO, BOUND).
    fn set_handler_r3(&mut self, h: u64)   { self.set(h, 0, GATE_INT_R3); }
}

#[repr(C, packed)] struct IdtPtr { limit: u16, base: u64 }
#[derive(Copy, Clone)] #[repr(C, packed)] struct GdtPtr { limit: u16, base: u64 }

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

// SAFETY: Todas estas estructuras se inicializan UNA SOLA VEZ durante
// init_idt() antes de que cualquier concurrencia sea posible. No necesitan
// Spinlock porque son read-only después del boot.
// [T-025 E3b] Una pila de #DF y una TSS por CPU (índice lógico de `percpu`).
pub const N_CPUS_TABLAS: usize = crate::arch::smp::MAX_CORES;
const TSS_VACIA: Tss = Tss {
    _res0:0, rsp:[0;3], _res1:0, ist:[0;7], _res2:0, _res3:0,
    iomap_base: core::mem::size_of::<Tss>() as u16,
};
// Sólo la pila de #DF del BSP es estática: el `.bss` del núcleo termina hoy en
// ~0x5DC000 físico y el cargador dejaba el `bootinfo` en 0x600000 (hoy 0xE00000); 16 pilas
// estáticas de 16 KiB lo pisaban (el asignador de marcos arrancaba con 0 marcos).
// Las de los AP se reservan en el montículo al arrancar cada AP.
static mut DF_STACK: Stack16K = Stack16K([0u8; 16384]);
static mut DF_STACK_AP: [u64; N_CPUS_TABLAS] = [0; N_CPUS_TABLAS];
static mut TSS: [Tss; N_CPUS_TABLAS] = [TSS_VACIA; N_CPUS_TABLAS];

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

// SAFETY: Init-once, ver nota arriba. GDT y GDT_PTR se escriben en init_idt()
// antes de habilitar interrupciones, luego son read-only.
const GDT_BASE: Gdt = Gdt {
    null:   0x0000_0000_0000_0000,
    code64: 0x00AF_9A00_0000_FFFF,
    data64: 0x00CF_9200_0000_FFFF,
    // DPL=3: access byte = 0xF2 (data, writable) / 0xFA (code, readable)
    user_data: 0x00CF_F200_0000_FFFF,
    user_code: 0x00AF_FA00_0000_FFFF,
    tss_low: 0, tss_high: 0,
};
// [T-025 E3b] Una GDT por CPU: los selectores 0x08..0x20 son idénticos en todas;
// sólo el descriptor de TSS (0x28) apunta a la TSS de esa CPU. Antes había una
// única TSS y los AP ni siquiera cargaban `tr` (no podían recibir una
// interrupción desde Ring 3 ni un #DF con pila propia).
static mut GDT: [Gdt; N_CPUS_TABLAS] = [GDT_BASE; N_CPUS_TABLAS];

// SAFETY: Init-once. GDT_PTR e IDT_PTR se escriben antes de lgdt/lidt.
static mut GDT_PTR: [GdtPtr; N_CPUS_TABLAS] = [GdtPtr { limit:0, base:0 }; N_CPUS_TABLAS];
static mut IDT_PTR: IdtPtr = IdtPtr { limit:0, base:0 };

// SAFETY: Init-once. IDT se escribe en init_idt() antes de habilitar IRQs.
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
    // T-025 SMP: stubs de IPI (sólo añadidos; véase isr.asm al final)
    pub fn ipi_stub_f0();
    pub fn ipi_stub_f1();
    pub fn ipi_stub_f2();
    pub fn ap_timer_handler();
    pub fn ipi_stub_espurio();
    fn irq0_handler();
    fn irq1_handler();
    fn irq12_handler();
    fn irq14_handler();
    fn irq15_handler();
    fn irq_stub_master();
    fn irq_stub_slave();
    fn syscall_entry();
    fn int80_handler();
}

/// TSS de la CPU que ejecuta (índice lógico por `percpu`).
pub fn get_tss_ptr() -> *mut Tss {
    let i = crate::arch::percpu::indice_actual().min(N_CPUS_TABLAS - 1);
    unsafe { core::ptr::addr_of_mut!(TSS[i]) }
}

/// [E3b] Dirección de la TSS de la CPU `i` (para `/proc/smp`: cada CPU debe
/// tener la suya, distinta de las demás).
pub fn tss_direccion(i: usize) -> u64 {
    unsafe { core::ptr::addr_of!(TSS[i.min(N_CPUS_TABLAS - 1)]) as u64 }
}

/// [E3b] Selector `tr` cargado en la CPU que ejecuta (0 si no hay TSS).
pub fn tr_actual() -> u16 {
    let tr: u16;
    unsafe { asm!("str {0:x}", out(reg) tr, options(nostack, nomem, preserves_flags)); }
    tr
}

/// [E3b] Rellena la TSS `i` (IST1 = pila de #DF propia, RSP0 = `rsp0`) y el
/// descriptor de TSS de la GDT `i`; deja `GDT_PTR[i]` listo para `lgdt`.
unsafe fn preparar_tablas_cpu(i: usize, rsp0: u64) {
    let i = i.min(N_CPUS_TABLAS - 1);
    let df_top = if i == 0 {
        (core::ptr::addr_of!(DF_STACK) as *const u8).add(core::mem::size_of::<Stack16K>()) as u64
    } else {
        if DF_STACK_AP[i] == 0 {
            if let Ok(l) = core::alloc::Layout::from_size_align(core::mem::size_of::<Stack16K>(), 16) {
                let p = alloc::alloc::alloc_zeroed(l);
                if !p.is_null() { DF_STACK_AP[i] = p as u64 + core::mem::size_of::<Stack16K>() as u64; }
            }
        }
        DF_STACK_AP[i]
    };
    TSS[i].ist[0] = df_top;
    TSS[i].rsp[0] = rsp0;
    let base  = core::ptr::addr_of!(TSS[i]) as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    GDT[i].code64    = 0x00AF_9A00_0000_FFFF;
    GDT[i].data64    = 0x00CF_9200_0000_FFFF;
    GDT[i].user_data = 0x00CF_F200_0000_FFFF;
    GDT[i].user_code = 0x00AF_FA00_0000_FFFF;
    GDT[i].tss_low =
          (limit  & 0x0000_FFFF)
        | ((base  & 0x00FF_FFFF) << 16)
        | 0x0000_8900_0000_0000_u64
        | ((limit & 0x000F_0000) << 32)
        | ((base  & 0xFF00_0000) << 32);
    GDT[i].tss_high = (base >> 32) & 0xFFFF_FFFF;
    GDT_PTR[i].limit = (core::mem::size_of::<Gdt>() - 1) as u16;
    GDT_PTR[i].base  = core::ptr::addr_of!(GDT[i]) as u64;
}

pub unsafe fn init_idt() {
    let kernel_stack_top = core::ptr::addr_of!(crate::__stack_top) as u64;

    // 1-2. [E3b] TSS/GDT de la CPU 0 (BSP): IST1 para #DF + RSP0 para ring 3 → ring 0.
    preparar_tablas_cpu(0, kernel_stack_top);

    // 3. Dump GDT content to serial for debugging
    {
        let gdt_addr = core::ptr::addr_of!(GDT[0]) as u64;
        let gdt_ptr = gdt_addr as *const u64;
        let hex = b"0123456789ABCDEF";
        let wb = |b: u8| { crate::drivers::serial::write_byte(b); };
        let wh = |b: u8| { wb(hex[(b>>4) as usize]); wb(hex[(b&0xF) as usize]); };
        let wq = |v: u64| { wh(((v>>56)&0xFF) as u8); wh(((v>>48)&0xFF) as u8); wh(((v>>40)&0xFF) as u8); wh(((v>>32)&0xFF) as u8); wh(((v>>24)&0xFF) as u8); wh(((v>>16)&0xFF) as u8); wh(((v>>8)&0xFF) as u8); wh((v&0xFF) as u8); };
        let waddr = |v: u64| { wh(((v>>60)&0xF) as u8); wh(((v>>56)&0xF) as u8); wh(((v>>52)&0xF) as u8); wh(((v>>48)&0xF) as u8); wh(((v>>44)&0xF) as u8); wh(((v>>40)&0xF) as u8); wh(((v>>36)&0xF) as u8); wh(((v>>32)&0xF) as u8); wh(((v>>28)&0xF) as u8); wh(((v>>24)&0xF) as u8); wh(((v>>20)&0xF) as u8); wh(((v>>16)&0xF) as u8); wh(((v>>12)&0xF) as u8); wh(((v>>8)&0xF) as u8); wh(((v>>4)&0xF) as u8); wh((v&0xF) as u8); };
        // First dump first qword of kernel at 0x200000
        let base: u64 = core::ptr::read_volatile(0x200000 as *const u64);
        wb(b'K'); wb(b'E'); wb(b'R'); wb(b'@'); wb(b'2'); wb(b'0'); wb(b'0'); wb(b'0'); wb(b'0'); wb(b'0'); wb(b'=');
        wq(base);
        wb(b'\n');
        // Dump STAGING buffer at 0x10000 to verify INT13h source
        let stage: u64 = core::ptr::read_volatile(0x10000 as *const u64);
        wb(b'S'); wb(b'T'); wb(b'G'); wb(b'@'); wb(b'1'); wb(b'0'); wb(b'0'); wb(b'0'); wb(b'0'); wb(b'=');
        wq(stage);
        wb(b'\n');
        // Dump STAGING buffer at GDT offset (compute dynamically)
        let stage_gdt_vma = 0x10000u64 + (gdt_addr - 0x200000u64);
        let stage_gdt_val: u64 = core::ptr::read_volatile(stage_gdt_vma as *const u64);
        wb(b'S'); wb(b'T'); wb(b'G'); wb(b'@'); wb(b'G'); wb(b'D'); wb(b'T'); wb(b'=');
        wq(stage_gdt_val);
        wb(b'\n');
        // Then dump GDT
        wb(b'G'); wb(b'D'); wb(b'T'); wb(b'@');
        waddr(gdt_addr);
        wb(b'=');
        for i in 0..7 {
            let val = core::ptr::read_volatile(gdt_ptr.add(i));
            wb(b' '); wq(val);
        }
        wb(b'\n');

        // Dump GDT_PTR before loading
        let ptr = core::ptr::addr_of!(GDT_PTR[0]);
        let limit: u16 = core::ptr::read_volatile(ptr as *const u16);
        let base: u64 = core::ptr::read_volatile((ptr as *const u8).add(2) as *const u64);
        wb(b'P'); wb(b'T'); wb(b'R'); wb(b'=');
        wh(((limit>>8)&0xFF) as u8); wh((limit&0xFF) as u8);
        wb(b':');
        wq(base);
        wb(b'\n');
    }

    // 4. Load GDT (7 entradas × 8 = 56 bytes, limit = 55)
    {
        let hex = b"0123456789ABCDEF";
        let wb = |b: u8| { crate::drivers::serial::write_byte(b); };
        let wh = |b: u8| { wb(hex[(b>>4) as usize]); wb(hex[(b&0xF) as usize]); };
        let wq = |v: u64| { wh(((v>>56)&0xFF) as u8); wh(((v>>48)&0xFF) as u8); wh(((v>>40)&0xFF) as u8); wh(((v>>32)&0xFF) as u8); wh(((v>>24)&0xFF) as u8); wh(((v>>16)&0xFF) as u8); wh(((v>>8)&0xFF) as u8); wh((v&0xFF) as u8); };
        let ptr = core::ptr::addr_of!(GDT_PTR[0]);
        let limit: u16 = core::ptr::read_volatile(ptr as *const u16);
        let base: u64 = core::ptr::read_volatile((ptr as *const u8).add(2) as *const u64);
        wb(b'N'); wb(b'E'); wb(b'W'); wb(b'P'); wb(b'T'); wb(b'R'); wb(b'=');
        wh(((limit>>8)&0xFF) as u8); wh((limit&0xFF) as u8);
        wb(b':');
        wq(base);
        wb(b'\n');
    }
    asm!("lgdt [{p}]", p = in(reg) core::ptr::addr_of!(GDT_PTR[0]),
         options(nostack, preserves_flags, readonly));

    // 5. Reload CS (far return: 0x08 → CS)
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
    macro_rules! h { ($f:expr) => { $f as *const () as u64 } }
    IDT[ 0].set_handler(h!(isr_0));
    IDT[ 1].set_handler(h!(isr_1));   // #DB (accesible también desde Ring 3)
    IDT[ 2].set_handler(h!(isr_2));
    IDT[ 3].set_handler_r3(h!(isr_3)); // #BP: INT3 del depurador en Ring 3 (DPL=3)
    IDT[ 4].set_handler_r3(h!(isr_4)); // #OF: INTO desde Ring 3
    IDT[ 5].set_handler_r3(h!(isr_5)); // #BR: BOUND desde Ring 3
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
    let irq0  = irq0_handler as *const () as u64;
    let irq_m = irq_stub_master as *const () as u64;
    let irq_s = irq_stub_slave as *const () as u64;
    IDT[0x20].set_handler(irq0);
    IDT[0x21].set_handler(irq1_handler as *const () as u64);
    for i in 0x22..=0x27_usize { IDT[i].set_handler(irq_m); }
    for i in 0x28..=0x2F_usize { IDT[i].set_handler(irq_s); }
    IDT[0x2C].set_handler(irq12_handler as *const () as u64);
    IDT[0x2E].set_handler(irq14_handler as *const () as u64);
    IDT[0x2F].set_handler(irq15_handler as *const () as u64);
    // [T-025 E3a] Puerta de INTERRUPCIÓN (0xEE: DPL3, IF=0 al entrar), no de
    // trampa (0xEF): con IF=1 una IRQ0 podía colarse ANTES de la primera
    // instrucción de `int80_handler` (su `swapgs` condicional), con CS=0x08 ya
    // en la pila; la IRQ no intercambiaba GS y corría el núcleo con el GS del
    // usuario (`gs_invalido` crecía ~1 por cada 100 ms de procesos con int 0x80).
    // `syscall_entry` ya entra con IF=0 por FMASK: mismo contrato para ambas.
    IDT[0x80].set(h!(int80_handler), 0, GATE_INT_R3);

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
    core::arch::asm!("out 0x21, al", in("al") 0xF8u8, options(nostack, nomem));  // Unmask IRQ0 + IRQ1 + IRQ2 (cascade)
    core::arch::asm!("out 0xA1, al", in("al") 0xAFu8, options(nostack, nomem)); // Unmask IRQ12 (bit4), IRQ14 (bit6), IRQ15 (bit7)

    // 13. MSR setup for syscall/sysret ([E3d] compartido con los AP)
    configurar_msrs_syscall(true);

    // 14. Enable interrupts
    asm!("sti", options(nostack, preserves_flags));
}


/// [T-025 E3d] Programa EFER.SCE, STAR, LSTAR y FMASK en ESTA CPU. Los MSR de
/// `syscall` son por núcleo: hasta E3d sólo el BSP los tenía, y el primer
/// `syscall` (musl/glibc) de un proceso en un AP saltaba a LSTAR=0 con CS=0 y
/// sin `swapgs` (`[GS!]` con RSP de usuario, `invalid_opcode` RIP=0x3). Los
/// binarios nativos (`int 0x80`) no lo delataban.
pub unsafe fn configurar_msrs_syscall(imprimir: bool) {
    // Enable SCE (System Call Extension) bit 0 in IA32_EFER (0xC000_0080)
    let efer_msr = 0xC000_0080u32;
    let mut efer_lo: u32;
    let mut efer_hi: u32;
    asm!("rdmsr",
         in("ecx") efer_msr, out("eax") efer_lo, out("edx") efer_hi,
         options(nostack, preserves_flags));
    efer_lo |= 1; // SCE = bit 0
    asm!("wrmsr",
         in("ecx") efer_msr, in("eax") efer_lo, in("edx") efer_hi,
         options(nostack, preserves_flags));

    // STAR[47:32] = KERNEL_CS(0x08) → SYSCALL loads CS=0x08, SS=0x08+8=0x10 ✓
    // STAR[63:48] = KERNEL_DS(0x10) → SYSRET loads CS=(0x10+16)|3=0x23, SS=(0x10+8)|3=0x1B ✓
    let star = (KERNEL_CS as u64) << 16 | (KERNEL_CS as u64) << 32
             | (KERNEL_DS as u64) << 48;
    // DEBUG: print star before wrmsr
    if imprimir {
    crate::drivers::serial::write_str("star=");
    let hex = b"0123456789ABCDEF";
    let hi = (star >> 32) as u32;
    let lo = star as u32;
    for sh in (0..8).rev() { let b = ((hi >> (sh*4)) & 0xF) as usize; crate::drivers::serial::write_byte(hex[b]); }
    crate::drivers::serial::write_byte(b':');
    for sl in (0..8).rev() { let b = ((lo >> (sl*4)) & 0xF) as usize; crate::drivers::serial::write_byte(hex[b]); }
    crate::drivers::serial::write_byte(b'\n');
    }
    let lstar = syscall_entry as *const () as u64;
    // [T-025 E3a] + TF(8), DF(10), NT(14), RF(16): con TF activo (paso a paso de
    // ptrace/dbg) el #DB saltaria en la primera instruccion de `syscall_entry`,
    // ANTES de su `swapgs`, con CS=0x08 y el GS del usuario. El TF del proceso
    // se conserva en el R11/RFLAGS del frame y lo restaura el iretq.
    let fmask = 0x47700u64;

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

}

// ─────────────────────────────────────────────────────────────────────────────
// T-025 SMP — SOLO AÑADIDOS. Vectores de IPI y carga de tablas en los AP.
// ─────────────────────────────────────────────────────────────────────────────
pub const VECTOR_IPI_PRUEBA: u8 = 0xF0;
pub const VECTOR_IPI_TLB: u8 = 0xF1;
pub const VECTOR_IPI_REPLANIFICAR: u8 = 0xF2;
/// [E3b] Temporizador LAPIC de cada AP (no es una IPI, pero comparte el stub).
pub const VECTOR_TEMPORIZADOR_LAPIC: u8 = 0xF3;
pub const VECTOR_IPI_ESPURIO: u8 = 0xFF;

/// Instala los vectores de IPI en la IDT compartida (DPL 0, puerta de
/// interrupción). Debe llamarse en el BSP tras `init_idt`, antes de arrancar
/// los AP. No toca ningún vector existente.
pub unsafe fn instalar_vectores_ipi() {
    macro_rules! h { ($f:expr) => { $f as *const () as u64 } }
    IDT[VECTOR_IPI_PRUEBA as usize].set_handler(h!(ipi_stub_f0));
    IDT[VECTOR_IPI_TLB as usize].set_handler(h!(ipi_stub_f1));
    IDT[VECTOR_IPI_REPLANIFICAR as usize].set_handler(h!(ipi_stub_f2));
    IDT[VECTOR_TEMPORIZADOR_LAPIC as usize].set_handler(h!(ap_timer_handler));
    IDT[VECTOR_IPI_ESPURIO as usize].set_handler(h!(ipi_stub_espurio));
}

/// [E3b] Carga en el núcleo actual (el AP de índice lógico `idx`) su GDT (con
/// su propia TSS: IST1 de #DF y RSP0 propios), recarga CS/DS/SS, carga `tr` y
/// la IDT compartida. `rsp0` = cima de la pila de núcleo de ese AP.
pub unsafe fn cargar_tablas_ap(idx: usize, rsp0: u64) {
    let i = idx.min(N_CPUS_TABLAS - 1);
    preparar_tablas_cpu(i, rsp0);
    asm!("lgdt [{p}]", p = in(reg) core::ptr::addr_of!(GDT_PTR[i]),
         options(nostack, preserves_flags, readonly));
    reload_segments();
    asm!(
        "mov ax, 0x10",
        "mov ds, ax", "mov es, ax", "mov ss, ax",
        "xor ax, ax",
        "mov fs, ax", "mov gs, ax",
        out("ax") _, options(nostack, preserves_flags)
    );
    asm!("ltr ax", in("ax") TSS_SEL, options(nostack, preserves_flags));
    configurar_msrs_syscall(false); // [E3d] puerta `syscall` también en el AP
    asm!("lidt [{p}]", p = in(reg) core::ptr::addr_of!(IDT_PTR),
         options(nostack, preserves_flags, readonly));
}
