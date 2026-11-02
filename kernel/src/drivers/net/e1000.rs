// kernel/src/drivers/net/e1000.rs — Intel 82540EM / 8254x Gigabit Ethernet Driver
#![allow(dead_code)]

use crate::arch::Spinlock;
use crate::drivers::bus::pci::{self, PciDevice};
use crate::drivers::serial;
use crate::mem::paging;

const REG_CTRL:    usize = 0x0000;
const REG_STATUS:  usize = 0x0008;
const REG_EECD:    usize = 0x0010;
const REG_EERD:    usize = 0x0014;
const REG_ICR:     usize = 0x00C0;
const REG_IMS:     usize = 0x00D0;
const REG_IMC:     usize = 0x00D8;
const REG_RCTL:    usize = 0x0100;
const REG_RDBAL:   usize = 0x2800;
const REG_RDBAH:   usize = 0x2804;
const REG_RDLEN:   usize = 0x2808;
const REG_RDH:     usize = 0x2810;
const REG_RDT:     usize = 0x2818;
const REG_TCTL:    usize = 0x0400;
const REG_TDBAL:   usize = 0x3800;
const REG_TDBAH:   usize = 0x3804;
const REG_TDLEN:   usize = 0x3808;
const REG_TDH:     usize = 0x3810;
const REG_TDT:     usize = 0x3818;
const REG_RAL:     usize = 0x5400;
const REG_RAH:     usize = 0x5404;

pub const NUM_RX_DESC: usize = 32;
pub const NUM_TX_DESC: usize = 16;
pub const PKT_BUF_SIZE: usize = 2048;

// [FRENTE B] Los anillos de descriptores DEBEN estar alineados a 16 bytes
// (RDBAL/TDBAL ignoran los 4 bits bajos; Intel 8254x §3.2.3 / §3.3.3). Sin
// `align(16)` la alineacion dependia de donde cayera el `static` en cada
// build y la recepcion quedaba muerta de forma intermitente.
#[repr(C, align(16))]
#[derive(Copy, Clone, Default)]
pub struct RxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

#[repr(C, align(16))]
#[derive(Copy, Clone, Default)]
pub struct TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

struct E1000State {
    mmio_base: usize,
    mac: [u8; 6],
    rx_descs: [RxDesc; NUM_RX_DESC],
    tx_descs: [TxDesc; NUM_TX_DESC],
    rx_buffers: [[u8; PKT_BUF_SIZE]; NUM_RX_DESC],
    tx_buffers: [[u8; PKT_BUF_SIZE]; NUM_TX_DESC],
    cur_rx: usize,
    cur_tx: usize,
    rx_packets: u64,
    tx_packets: u64,
    active: bool,
}

static E1000: Spinlock<E1000State> = Spinlock::new(E1000State {
    mmio_base: 0,
    mac: [0u8; 6],
    rx_descs: [RxDesc { addr: 0, length: 0, checksum: 0, status: 0, errors: 0, special: 0 }; NUM_RX_DESC],
    tx_descs: [TxDesc { addr: 0, length: 0, cso: 0, cmd: 0, status: 0, css: 0, special: 0 }; NUM_TX_DESC],
    rx_buffers: [[0u8; PKT_BUF_SIZE]; NUM_RX_DESC],
    tx_buffers: [[0u8; PKT_BUF_SIZE]; NUM_TX_DESC],
    cur_rx: 0,
    cur_tx: 0,
    rx_packets: 0,
    tx_packets: 0,
    active: false,
});

impl E1000State {
    #[inline(always)]
    unsafe fn read32(&self, reg: usize) -> u32 {
        core::ptr::read_volatile((self.mmio_base + reg) as *const u32)
    }

    #[inline(always)]
    unsafe fn write32(&self, reg: usize, val: u32) {
        core::ptr::write_volatile((self.mmio_base + reg) as *mut u32, val);
    }

    fn read_eeprom(&self, addr: u8) -> u16 {
        unsafe {
            self.write32(REG_EERD, 1 | ((addr as u32) << 8));
            let mut timeout = 100_000;
            while timeout > 0 {
                let v = self.read32(REG_EERD);
                if v & (1 << 4) != 0 {
                    return ((v >> 16) & 0xFFFF) as u16;
                }
                timeout -= 1;
            }
        }
        0
    }
}

pub fn init(dev: &PciDevice) -> bool {
    serial::write_str("[NET] Probing Intel e1000 @ PCI ");
    serial::write_usize(dev.bus as usize);
    serial::write_str(":");
    serial::write_usize(dev.device as usize);
    serial::write_str("...\n");

    let bar0 = unsafe { pci::pci_read32(dev.bus, dev.device, dev.function, 0x10) };
    if bar0 & 1 != 0 {
        serial::write_str("[NET] ERR: BAR0 is I/O port, expected MMIO\n");
        return false;
    }
    let mmio_phys = (bar0 & !0xF) as usize;
    if mmio_phys == 0 {
        serial::write_str("[NET] ERR: invalid BAR0 address 0\n");
        return false;
    }

    // Enable PCI Bus Master + Memory Space
    unsafe {
        let cmd = pci::pci_read32(dev.bus, dev.device, dev.function, 0x04);
        pci::pci_write16(dev.bus, dev.device, dev.function, 0x04, (cmd | 0x0007) as u16);
    }

    // Map 128 KB MMIO into kernel page table
    let cr3 = paging::read_cr3();
    for i in 0..(128 * 1024 / 4096) {
        let p = mmio_phys + i * 4096;
        let _ = paging::map_page(cr3, p, p, paging::PRESENT | paging::WRITABLE | paging::CACHE_DISABLE);
    }

    let mut guard = E1000.lock();
    guard.mmio_base = mmio_phys;

    // Read MAC Address from RAL/RAH or EEPROM
    unsafe {
        let ral = guard.read32(REG_RAL);
        let rah = guard.read32(REG_RAH);
        if ral != 0 || (rah & 0xFFFF) != 0 {
            guard.mac[0] = (ral & 0xFF) as u8;
            guard.mac[1] = ((ral >> 8) & 0xFF) as u8;
            guard.mac[2] = ((ral >> 16) & 0xFF) as u8;
            guard.mac[3] = ((ral >> 24) & 0xFF) as u8;
            guard.mac[4] = (rah & 0xFF) as u8;
            guard.mac[5] = ((rah >> 8) & 0xFF) as u8;
        } else {
            let w0 = guard.read_eeprom(0);
            let w1 = guard.read_eeprom(1);
            let w2 = guard.read_eeprom(2);
            guard.mac[0] = (w0 & 0xFF) as u8;
            guard.mac[1] = (w0 >> 8) as u8;
            guard.mac[2] = (w1 & 0xFF) as u8;
            guard.mac[3] = (w1 >> 8) as u8;
            guard.mac[4] = (w2 & 0xFF) as u8;
            guard.mac[5] = (w2 >> 8) as u8;
        }
    }

    // Initialize RX Ring
    let rx_descs_ptr = paging::virt_to_phys(guard.rx_descs.as_ptr() as usize) as u64; // [CICLO 49] la imagen vive en la mitad superior: al NIC se le da la FÍSICA
    for i in 0..NUM_RX_DESC {
        guard.rx_descs[i].addr = paging::virt_to_phys(guard.rx_buffers[i].as_ptr() as usize) as u64;
        guard.rx_descs[i].status = 0;
    }

    unsafe {
        guard.write32(REG_RDBAL, rx_descs_ptr as u32);
        guard.write32(REG_RDBAH, (rx_descs_ptr >> 32) as u32);
        guard.write32(REG_RDLEN, (NUM_RX_DESC * core::mem::size_of::<RxDesc>()) as u32);
        guard.write32(REG_RDH, 0);
        guard.write32(REG_RDT, (NUM_RX_DESC - 1) as u32);
        // RCTL: EN (1<<1) | MPE (1<<4, multidifusión: NDP/RA de IPv6 llegan a 33:33:xx) | BAM (1<<15) | SECRC (1<<26)
        guard.write32(REG_RCTL, (1 << 1) | (1 << 4) | (1 << 15) | (1 << 26));
    }

    // Initialize TX Ring
    let tx_descs_ptr = paging::virt_to_phys(guard.tx_descs.as_ptr() as usize) as u64;
    for i in 0..NUM_TX_DESC {
        guard.tx_descs[i].addr = paging::virt_to_phys(guard.tx_buffers[i].as_ptr() as usize) as u64;
        guard.tx_descs[i].status = 1; // Mark as done initially
        guard.tx_descs[i].cmd = 0;
    }

    unsafe {
        guard.write32(REG_TDBAL, tx_descs_ptr as u32);
        guard.write32(REG_TDBAH, (tx_descs_ptr >> 32) as u32);
        guard.write32(REG_TDLEN, (NUM_TX_DESC * core::mem::size_of::<TxDesc>()) as u32);
        guard.write32(REG_TDH, 0);
        guard.write32(REG_TDT, 0);
        // TCTL: EN (1<<1) | PSP (1<<3) | CT=0x10 | COLD=0x40
        guard.write32(REG_TCTL, (1 << 1) | (1 << 3) | (0x10 << 4) | (0x40 << 12));
    }

    guard.active = true;

    serial::write_str("[NET] e1000 initialized successfully! MAC: ");
    for i in 0..6 {
        let b = guard.mac[i];
        let h = b"0123456789ABCDEF";
        serial::write_byte(h[(b >> 4) as usize]);
        serial::write_byte(h[(b & 0xF) as usize]);
        if i < 5 { serial::write_byte(b':'); }
    }
    serial::write_str("\n");

    true
}

pub fn get_mac() -> [u8; 6] {
    E1000.lock().mac
}

pub fn is_active() -> bool {
    E1000.lock().active
}

pub fn get_stats() -> (u64, u64) {
    let guard = E1000.lock();
    (guard.rx_packets, guard.tx_packets)
}

pub fn send_packet(packet: &[u8]) -> Result<(), &'static str> {
    if packet.len() > PKT_BUF_SIZE {
        return Err("Packet exceeds max buffer size");
    }

    let mut guard = E1000.lock();
    if !guard.active {
        return Err("e1000 not active");
    }

    let cur = guard.cur_tx;
    guard.tx_buffers[cur][..packet.len()].copy_from_slice(packet);
    guard.tx_descs[cur].length = packet.len() as u16;
    // CMD: EOP (1<<0) | IFCS (1<<1) | RS (1<<3)
    guard.tx_descs[cur].cmd = (1 << 0) | (1 << 1) | (1 << 3);
    guard.tx_descs[cur].status = 0;

    let next_tx = (cur + 1) % NUM_TX_DESC;
    guard.cur_tx = next_tx;
    guard.tx_packets += 1;

    unsafe {
        guard.write32(REG_TDT, next_tx as u32);
    }

    Ok(())
}

pub fn poll_packet(buf: &mut [u8]) -> Option<usize> {
    let mut guard = E1000.lock();
    if !guard.active { return None; }

    let cur = guard.cur_rx;
    // Lectura volatil: el descriptor lo escribe el NIC por DMA.
    let status = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(guard.rx_descs[cur].status)) };
    if status & 1 == 0 {
        // No packet ready (Descriptor Done DD=1 not set)
        return None;
    }

    let len = (guard.rx_descs[cur].length as usize).min(buf.len());
    buf[..len].copy_from_slice(&guard.rx_buffers[cur][..len]);
    guard.rx_descs[cur].status = 0;

    unsafe {
        guard.write32(REG_RDT, cur as u32);
    }

    guard.cur_rx = (cur + 1) % NUM_RX_DESC;
    guard.rx_packets += 1;
    Some(len)
}

// Garantias de layout exigidas por el hardware (16 bytes por descriptor).
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::align_of::<RxDesc>() == 16);
