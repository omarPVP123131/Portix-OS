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
                    let id = pci_read32(b, d, 0, 0);
                    let vendor = (id & 0xFFFF) as u16;
                    if vendor == 0xFFFF { continue; }

                    let header = pci_read8(b, d, 0, 0x0E);
                    let max_func: u8 = if header & 0x80 != 0 { 8 } else { 1 };

                    for f in 0u8..max_func {
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