// kernel/src/pci.rs — PORTIX PCI Bus Enumeration
#![allow(dead_code)]

const PCI_ADDR: u16 = 0xCF8;
const PCI_DATA: u16 = 0xCFC;

pub const MAX_PCI_DEVICES: usize = 64;

#[derive(Clone, Copy)]
pub struct PciDevice {
    pub bus:        u8,
    pub device:     u8,
    pub function:   u8,
    pub vendor_id:  u16,
    pub device_id:  u16,
    pub class_code: u8,
    pub subclass:   u8,
    pub prog_if:    u8,
    pub header_type: u8,
    pub irq_line:   u8,
}

impl PciDevice {
    pub const fn empty() -> Self {
        PciDevice { bus:0, device:0, function:0,
            vendor_id: 0xFFFF, device_id: 0xFFFF,
            class_code:0, subclass:0, prog_if:0,
            header_type:0, irq_line:0xFF }
    }

    pub fn class_name(&self) -> &'static str {
        match self.class_code {
            0x00 => "Unclassified",
            0x01 => match self.subclass {
                0x01 => "IDE Controller",
                0x06 => "SATA (AHCI)",
                0x08 => "NVM Express",
                _ => "Mass Storage",
            },
            0x02 => "Network Controller",
            0x03 => match self.subclass {
                0x00 => "VGA Controller",
                0x01 => "XGA Controller",
                0x02 => "3D Controller",
                _ => "Display Controller",
            },
            0x04 => "Multimedia Controller",
            0x05 => "Memory Controller",
            0x06 => match self.subclass {
                0x00 => "Host Bridge",
                0x01 => "ISA Bridge",
                0x04 => "PCI-PCI Bridge",
                _ => "Bridge Device",
            },
            0x07 => "Communication Controller",
            0x08 => "System Peripheral",
            0x09 => "Input Device",
            0x0C => match self.subclass {
                0x03 => "USB Controller",
                0x05 => "SMBus",
                _ => "Serial Bus Controller",
            },
            0x0D => "Wireless Controller",
            0x10 => "Encryption Controller",
            0x11 => "Signal Processing",
            _ => "Unknown Device",
        }
    }

    pub fn vendor_name(&self) -> &'static str {
        match self.vendor_id {
            0x8086 => "Intel",
            0x1022 => "AMD",
            0x10DE => "NVIDIA",
            0x1002 => "AMD/ATI",
            0x14E4 => "Broadcom",
            0x1AF4 => "VirtIO",
            0x1234 => "QEMU/Bochs",
            0x106B => "Apple",
            0x15AD => "VMware",
            0x80EE => "VirtualBox",
            _ => "Unknown",
        }
    }
}

#[inline(always)]
unsafe fn outl(p: u16, v: u32) {
    core::arch::asm!("out dx, eax", in("dx") p, in("eax") v, options(nostack, nomem));
}
#[inline(always)]
unsafe fn inl(p: u16) -> u32 {
    let v: u32;
    core::arch::asm!("in eax, dx", out("eax") v, in("dx") p, options(nostack, nomem));
    v
}

fn make_addr(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    0x8000_0000
    | ((bus  as u32) << 16)
    | ((dev  as u32) << 11)
    | ((func as u32) <<  8)
    | ((reg  as u32) &  0xFC)
}

pub unsafe fn pci_read32(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    outl(PCI_ADDR, make_addr(bus, dev, func, reg));
    inl(PCI_DATA)
}

pub unsafe fn pci_read8(bus: u8, dev: u8, func: u8, reg: u8) -> u8 {
    let v = pci_read32(bus, dev, func, reg & !3);
    (v >> ((reg & 3) * 8)) as u8
}

/// Lee la dirección base de un BAR PCI (offset 0x10..0x24).
/// Maneja BARs 32-bit y 64-bit (pair). Devuelve 0 si no es MMIO.
unsafe fn pci_read_bar_base(bus: u8, dev: u8, func: u8, reg: u8) -> u64 {
    let bar = pci_read32(bus, dev, func, reg);
    if bar & 1 != 0 { return 0; }          // I/O BAR, skip
    if (bar >> 1) & 0x3 == 0x2 {           // 64-bit
        let hi = pci_read32(bus, dev, func, reg + 4);
        ((hi as u64) << 32) | (bar as u64 & !0xF)
    } else {                                 // 32-bit
        (bar as u64) & !0xF
    }
}

/// Escanea PCI buscando un controlador VGA (class=0x03, subclass=0x00)
/// y devuelve la dirección física del framebuffer.
/// Itera los 6 BARs (0x10..0x24) para hallar el primer MMIO.
/// VirtualBox tiene el framebuffer en BAR2 (BAR0/BAR1 son I/O).
/// Usado como fallback cuando VESA/legacy no provee framebuffer.
pub fn pci_find_vga_framebuffer() -> u64 {
    unsafe {
        for b in 0u8..=255u8 {
            for d in 0u8..32u8 {
                for f in 0u8..8u8 {
                    let fid = pci_read32(b, d, f, 0);
                    if (fid & 0xFFFF) as u16 == 0xFFFF { continue; }
                    let cls = pci_read32(b, d, f, 0x08);
                    if (cls >> 24) as u8 != 0x03 || ((cls >> 16) & 0xFF) as u8 != 0x00 {
                        continue;
                    }
                    // VGA controller found — scan all 6 BARs for first MMIO
                    for reg in (0x10u8..=0x24u8).step_by(4) {
                        let a = pci_read_bar_base(b, d, f, reg);
                        if a != 0 { return a; }
                    }
                }
            }
        }
    }
    0
}

pub struct PciBus {
    pub devices: [PciDevice; MAX_PCI_DEVICES],
    pub count:   usize,
}

impl PciBus {
    pub fn scan() -> Self {
        let mut bus = PciBus {
            devices: [PciDevice::empty(); MAX_PCI_DEVICES],
            count: 0,
        };
        unsafe {
            'outer: for b in 0u8..=255u8 {
                for d in 0u8..32u8 {
                    for f in 0u8..8u8 {
                        let fid = pci_read32(b, d, f, 0);
                        let fvendor = (fid & 0xFFFF) as u16;
                        if fvendor == 0xFFFF { continue; }

                        let cls   = pci_read32(b, d, f, 0x08);
                        let irqr  = pci_read32(b, d, f, 0x3C);
                        if bus.count >= MAX_PCI_DEVICES { break 'outer; }
                        bus.devices[bus.count] = PciDevice {
                            bus: b, device: d, function: f,
                            vendor_id:  fvendor,
                            device_id:  (fid >> 16) as u16,
                            class_code: (cls >> 24) as u8,
                            subclass:   (cls >> 16) as u8,
                            prog_if:    (cls >>  8) as u8,
                            header_type: pci_read8(b, d, f, 0x0E),
                            irq_line:   (irqr & 0xFF) as u8,
                        };
                        bus.count += 1;
                    }
                }
            }
        }
        bus
    }
}