// kernel/src/idt.rs
use core::arch::asm;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    zero: u8,
    type_attr: u8,
    offset_high: u16,
}
impl IdtEntry {
    const fn new() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            zero: 0,
            type_attr: 0,
            offset_high: 0,
        }
    }

    fn set_handler(&mut self, handler: u32, selector: u16) {
        self.offset_low = (handler & 0xFFFF) as u16;
        self.offset_high = ((handler >> 16) & 0xFFFF) as u16;
        self.selector = selector;
        self.zero = 0;
        self.type_attr = 0x8E;
    }
}

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u32,
}

#[no_mangle]
static mut IDT: [IdtEntry; 256] = [IdtEntry::new(); 256];

#[no_mangle]
static mut IDT_DESCRIPTOR: IdtDescriptor = IdtDescriptor {
    limit: 0,
    base: 0,
};

extern "C" {
    fn isr_0();
    fn isr_5();
    fn isr_6();
    fn isr_8();
    fn isr_13();
}

pub unsafe fn init_idt() {
    IDT[0].set_handler(isr_0 as *const () as u32, 0x08);
    IDT[5].set_handler(isr_5 as *const () as u32, 0x08);
    IDT[6].set_handler(isr_6 as *const () as u32, 0x08);
    IDT[8].set_handler(isr_8 as *const () as u32, 0x08);
    IDT[13].set_handler(isr_13 as *const () as u32, 0x08);

    IDT_DESCRIPTOR.limit =
        (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16;

    IDT_DESCRIPTOR.base =
        core::ptr::addr_of!(IDT) as u32;

    asm!(
        "lidt [{0}]",
        in(reg) &raw const IDT_DESCRIPTOR,
        options(nostack, preserves_flags)
    );

    verify_idt_loaded();
}

unsafe fn verify_idt_loaded() {
    let mut actual_desc = IdtDescriptor { limit: 0, base: 0 };

    asm!(
        "sidt [{0}]",
        in(reg) &mut actual_desc,
        options(nostack, preserves_flags)
    );
let _vga = 0xB8000 as *mut u16;

    write_string_vga(b"IDT Set: Base=0x", 160 * 3 + 0, 0x0E);
    write_hex32_vga(IDT_DESCRIPTOR.base, 160 * 3 + 16);
    write_string_vga(b" Limit=0x", 160 * 3 + 24, 0x0E);
    write_hex16_vga(IDT_DESCRIPTOR.limit, 160 * 3 + 33);

    write_string_vga(b"IDT Now: Base=0x", 160 * 4 + 0, 0x0A);
    write_hex32_vga(actual_desc.base, 160 * 4 + 16);
    write_string_vga(b" Limit=0x", 160 * 4 + 24, 0x0A);
    write_hex16_vga(actual_desc.limit, 160 * 4 + 33);

    if actual_desc.base == IDT_DESCRIPTOR.base
        && actual_desc.limit == IDT_DESCRIPTOR.limit
    {
        write_string_vga(b" [OK]", 160 * 4 + 38, 0x0A);
    } else {
        write_string_vga(b" [FAIL]", 160 * 4 + 38, 0x0C);
    }
}

unsafe fn write_string_vga(s: &[u8], offset: usize, color: u8) {
    let vga = 0xB8000 as *mut u16;
    for (i, &ch) in s.iter().enumerate() {
        *vga.add(offset + i) =
            ((color as u16) << 8) | (ch as u16);
    }
}

unsafe fn write_hex32_vga(value: u32, offset: usize) {
    let vga = 0xB8000 as *mut u16;
    let hex = b"0123456789ABCDEF";
    for i in 0..8 {
        let nibble =
            ((value >> (28 - i * 4)) & 0xF) as usize;
        *vga.add(offset + i) =
            (0x0F << 8) | (hex[nibble] as u16);
    }
}

unsafe fn write_hex16_vga(value: u16, offset: usize) {
    let vga = 0xB8000 as *mut u16;
    let hex = b"0123456789ABCDEF";
    for i in 0..4 {
        let nibble =
            ((value >> (12 - i * 4)) & 0xF) as usize;
        *vga.add(offset + i) =
            (0x0F << 8) | (hex[nibble] as u16);
    }
}
