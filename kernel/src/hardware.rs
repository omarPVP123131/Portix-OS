// kernel/src/hardware.rs - PORTIX Hardware Detection Layer
// Detección universal: CPU (CPUID), Discos (ATA IDENTIFY), RAM (E820), Display
#![allow(dead_code)]

// ── Port I/O helpers ──────────────────────────────────────────────────────────
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nostack, nomem));
    v
}
#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, nomem));
}
#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    let v: u16;
    core::arch::asm!("in ax, dx", out("ax") v, in("dx") port, options(nostack, nomem));
    v
}
#[inline(always)]
unsafe fn io_wait() {
    outb(0x80, 0);
}

// ── CPUID wrapper ─────────────────────────────────────────────────────────────
#[derive(Default, Clone, Copy)]
struct CpuidResult { eax: u32, ebx: u32, ecx: u32, edx: u32 }

unsafe fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    // rbx is reserved by LLVM as the base pointer in some codegen configs.
    // We must save/restore it manually and move EBX out through a general reg.
    core::arch::asm!(
        "push rbx",
        "cpuid",
        "mov {tmp:e}, ebx",
        "pop rbx",
        inout("eax") leaf    => eax,
        inout("ecx") subleaf => ecx,
        tmp = out(reg) ebx,
        out("edx") edx,
        options(nostack, nomem)
    );
    CpuidResult { eax, ebx, ecx, edx }
}

// ── CPU Info ─────────────────────────────────────────────────────────────────
pub struct CpuInfo {
    pub vendor:        [u8; 13],  // "AuthenticAMD\0" / "GenuineIntel\0"
    pub brand:         [u8; 49],  // "AMD Ryzen 7 5600X...\0"
    pub logical_cores: u8,
    pub physical_cores: u8,
    pub base_mhz:      u32,       // MHz
    pub max_mhz:       u32,       // MHz
    pub has_sse2:      bool,
    pub has_sse4:      bool,
    pub has_avx:       bool,
    pub has_avx2:      bool,
    pub has_aes:       bool,
    pub max_leaf:      u32,
    pub max_ext_leaf:  u32,
}

impl CpuInfo {
    pub fn detect() -> Self {
        let mut info = CpuInfo {
            vendor: [0u8; 13],
            brand:  [0u8; 49],
            logical_cores:  1,
            physical_cores: 1,
            base_mhz:  0,
            max_mhz:   0,
            has_sse2:  false,
            has_sse4:  false,
            has_avx:   false,
            has_avx2:  false,
            has_aes:   false,
            max_leaf:      0,
            max_ext_leaf:  0,
        };

        unsafe {
            // ── Leaf 0: vendor string + max leaf ────────────────────────────
            let l0 = cpuid(0, 0);
            info.max_leaf = l0.eax;
            // EBX:EDX:ECX → vendor
            let v = &mut info.vendor;
            let b = l0.ebx.to_le_bytes();
            let d = l0.edx.to_le_bytes();
            let c = l0.ecx.to_le_bytes();
            v[0]=b[0]; v[1]=b[1]; v[2]=b[2];  v[3]=b[3];
            v[4]=d[0]; v[5]=d[1]; v[6]=d[2];  v[7]=d[3];
            v[8]=c[0]; v[9]=c[1]; v[10]=c[2]; v[11]=c[3];
            v[12]=0;

            // ── Leaf 1: features + logical core count ────────────────────────
            if info.max_leaf >= 1 {
                let l1 = cpuid(1, 0);
                // EBX[23:16] = logical processors in package
                info.logical_cores = ((l1.ebx >> 16) & 0xFF) as u8;
                if info.logical_cores == 0 { info.logical_cores = 1; }
                // EDX features
                info.has_sse2 = (l1.edx >> 26) & 1 != 0;
                // ECX features
                info.has_sse4 = (l1.ecx >> 19) & 1 != 0;
                info.has_avx  = (l1.ecx >> 28) & 1 != 0;
                info.has_aes  = (l1.ecx >> 25) & 1 != 0;
            }

            // ── Leaf 7: AVX2 ─────────────────────────────────────────────────
            if info.max_leaf >= 7 {
                let l7 = cpuid(7, 0);
                info.has_avx2 = (l7.ebx >> 5) & 1 != 0;
            }

            // ── Leaf 0x16: freq info (Intel mainly) ──────────────────────────
            if info.max_leaf >= 0x16 {
                let l16 = cpuid(0x16, 0);
                if l16.eax != 0 { info.base_mhz = l16.eax & 0xFFFF; }
                if l16.ebx != 0 { info.max_mhz  = l16.ebx & 0xFFFF; }
            }

            // ── Extended leaves ───────────────────────────────────────────────
            let le = cpuid(0x80000000, 0);
            info.max_ext_leaf = le.eax;

            // ── Extended leaf 0x80000001: core count AMD ─────────────────────
            if info.max_ext_leaf >= 0x80000008 {
                let l88 = cpuid(0x80000008, 0);
                // ECX[7:0] = logical core count - 1 (AMD)
                let nc = (l88.ecx & 0xFF) as u8 + 1;
                if nc > 1 { info.logical_cores = nc; }
                // ECX[15:8] = threads per core (AMD Zen 2+)
                let tpc = ((l88.ecx >> 8) & 0xFF) as u8;
                if tpc > 0 {
                    info.physical_cores = nc / (tpc + 1);
                } else {
                    info.physical_cores = nc / 2; // assume HT
                }
                if info.physical_cores == 0 { info.physical_cores = 1; }
            } else {
                info.physical_cores = (info.logical_cores / 2).max(1);
            }

            // ── Brand string (leaves 0x80000002-4) ───────────────────────────
            if info.max_ext_leaf >= 0x80000004 {
                let mut brand = [0u8; 48];
                for i in 0u32..3 {
                    let r = cpuid(0x80000002 + i, 0);
                    let base = (i * 16) as usize;
                    let bytes_a = r.eax.to_le_bytes();
                    let bytes_b = r.ebx.to_le_bytes();
                    let bytes_c = r.ecx.to_le_bytes();
                    let bytes_d = r.edx.to_le_bytes();
                    brand[base..base+4].copy_from_slice(&bytes_a);
                    brand[base+4..base+8].copy_from_slice(&bytes_b);
                    brand[base+8..base+12].copy_from_slice(&bytes_c);
                    brand[base+12..base+16].copy_from_slice(&bytes_d);
                }
                // Trim leading spaces
                let start = brand.iter().position(|&b| b != b' ').unwrap_or(0);
                let trimmed = &brand[start..];
                let len = trimmed.len().min(48);
                info.brand[..len].copy_from_slice(&trimmed[..len]);
                info.brand[48] = 0;

                // Parse frequency from brand string if not from leaf 0x16
                if info.max_mhz == 0 {
                    info.max_mhz = parse_freq_from_brand(&info.brand);
                }
                if info.base_mhz == 0 {
                    info.base_mhz = info.max_mhz;
                }
            }
        }

        info
    }

    /// Returns brand string as &str (trimmed)
    pub fn brand_str(&self) -> &str {
        let end = self.brand.iter().position(|&b| b == 0).unwrap_or(48);
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown CPU")
    }

    /// Returns vendor string as &str
    pub fn vendor_str(&self) -> &str {
        let end = self.vendor.iter().position(|&b| b == 0).unwrap_or(12);
        core::str::from_utf8(&self.vendor[..end]).unwrap_or("Unknown")
    }

    /// Short vendor name: "AMD" / "Intel" / "Unknown"
    pub fn vendor_short(&self) -> &str {
        let v = self.vendor_str();
        if v.contains("AMD")   { "AMD"   }
        else if v.contains("Intel") { "Intel" }
        else { v }
    }
}

/// Parse "3.70GHz" or "4.30GHz" or "3600MHz" from brand string → MHz
fn parse_freq_from_brand(brand: &[u8; 49]) -> u32 {
    // Find "GHz" or "MHz" in brand string
    let s = match core::str::from_utf8(brand) { Ok(s) => s, Err(_) => return 0 };
    // Find GHz pattern: digits + optional dot + digits + "GHz"
    let bytes = s.as_bytes();
    let len = bytes.len();
    for i in 0..len.saturating_sub(3) {
        if &bytes[i..i+3] == b"GHz" {
            // Parse backwards from i
            let mut j = i;
            while j > 0 && (bytes[j-1].is_ascii_digit() || bytes[j-1] == b'.') {
                j -= 1;
            }
            // Parse "3.70" or "4300" style
            let num_str = core::str::from_utf8(&bytes[j..i]).unwrap_or("");
            return parse_ghz_str(num_str);
        }
        if i + 3 < len && &bytes[i..i+3] == b"MHz" {
            let mut j = i;
            while j > 0 && bytes[j-1].is_ascii_digit() { j -= 1; }
            let num_str = core::str::from_utf8(&bytes[j..i]).unwrap_or("");
            return parse_u32_str(num_str);
        }
    }
    0
}

fn parse_ghz_str(s: &str) -> u32 {
    // "3.70" → 3700, "4.3" → 4300, "3" → 3000
    let b = s.as_bytes();
    let dot_pos = b.iter().position(|&c| c == b'.');
    if let Some(d) = dot_pos {
        let int_part = parse_u32_str(core::str::from_utf8(&b[..d]).unwrap_or("0"));
        let frac_bytes = &b[d+1..];
        let frac_len = frac_bytes.len().min(3);
        let frac_str = core::str::from_utf8(&frac_bytes[..frac_len]).unwrap_or("0");
        let frac = parse_u32_str(frac_str);
        // Normalize to 3 decimal places
        let multiplier = match frac_len { 1 => 100, 2 => 10, _ => 1 };
        return int_part * 1000 + frac * multiplier;
    }
    parse_u32_str(s) * 1000
}

fn parse_u32_str(s: &str) -> u32 {
    let mut n = 0u32;
    for c in s.bytes() {
        if c >= b'0' && c <= b'9' {
            n = n.saturating_mul(10).saturating_add((c - b'0') as u32);
        }
    }
    n
}

// ── Disk Info (ATA IDENTIFY) ──────────────────────────────────────────────────
// Soporta hasta 4 unidades: Primary Master/Slave, Secondary Master/Slave
pub const MAX_DISKS: usize = 4;

#[derive(Clone, Copy)]
pub struct DiskInfo {
    pub present:  bool,
    pub is_atapi: bool,        // CD-ROM / optical
    pub model:    [u8; 41],    // 40 char model + \0
    pub serial:   [u8; 21],    // 20 char serial + \0
    pub size_mb:  u64,         // MiB
    pub lba48:    bool,
    pub bus:      u8,          // 0=Primary, 1=Secondary
    pub drive:    u8,          // 0=Master, 1=Slave
}

impl DiskInfo {
    const fn empty() -> Self {
        DiskInfo {
            present:  false,
            is_atapi: false,
            model:    [0u8; 41],
            serial:   [0u8; 21],
            size_mb:  0,
            lba48:    false,
            bus:      0,
            drive:    0,
        }
    }
    pub fn model_str(&self) -> &str {
        let end = self.model.iter().position(|&b| b == 0).unwrap_or(40);
        core::str::from_utf8(&self.model[..end]).unwrap_or("Unknown")
    }
    pub fn serial_str(&self) -> &str {
        let end = self.serial.iter().position(|&b| b == 0).unwrap_or(20);
        core::str::from_utf8(&self.serial[..end]).unwrap_or("N/A")
    }
}

// ATA register offsets from base port
const ATA_REG_DATA:    u16 = 0;
const ATA_REG_ERROR:   u16 = 1;
const ATA_REG_COUNT:   u16 = 2;
const ATA_REG_LBA_LO:  u16 = 3;
const ATA_REG_LBA_MID: u16 = 4;
const ATA_REG_LBA_HI:  u16 = 5;
const ATA_REG_DRIVE:   u16 = 6;
const ATA_REG_STATUS:  u16 = 7;
const ATA_REG_CMD:     u16 = 7;

const ATA_STATUS_BSY:  u8 = 0x80;
const ATA_STATUS_DRQ:  u8 = 0x08;
const ATA_STATUS_ERR:  u8 = 0x01;

const ATA_CMD_IDENTIFY:       u8 = 0xEC;
const ATA_CMD_IDENTIFY_PACKET: u8 = 0xA1;

unsafe fn ata_wait_bsy(base: u16, timeout: u32) -> bool {
    for _ in 0..timeout {
        if inb(base + ATA_REG_STATUS) & ATA_STATUS_BSY == 0 { return true; }
        io_wait();
    }
    false
}

unsafe fn ata_wait_drq(base: u16, timeout: u32) -> bool {
    for _ in 0..timeout {
        let st = inb(base + ATA_REG_STATUS);
        if st & ATA_STATUS_ERR != 0 { return false; }
        if st & ATA_STATUS_DRQ != 0 { return true; }
        io_wait();
    }
    false
}

/// Detect one ATA drive. Returns None if not present.
unsafe fn ata_identify(base: u16, ctrl: u16, drive: u8) -> Option<DiskInfo> {
    // Select drive
    outb(base + ATA_REG_DRIVE, 0xA0 | ((drive & 1) << 4));
    io_wait(); io_wait(); io_wait(); io_wait(); // 400ns

    // Soft reset
    outb(ctrl, 0x04); io_wait(); io_wait();
    outb(ctrl, 0x00); io_wait(); io_wait();

    // Select drive again after reset
    outb(base + ATA_REG_DRIVE, 0xA0 | ((drive & 1) << 4));
    io_wait(); io_wait(); io_wait(); io_wait();

    if !ata_wait_bsy(base, 100_000) { return None; }

    // Check if drive exists (floating bus = 0xFF)
    let status = inb(base + ATA_REG_STATUS);
    if status == 0xFF || status == 0x7F { return None; }

    // Send IDENTIFY
    outb(base + ATA_REG_COUNT,   0);
    outb(base + ATA_REG_LBA_LO,  0);
    outb(base + ATA_REG_LBA_MID, 0);
    outb(base + ATA_REG_LBA_HI,  0);
    outb(base + ATA_REG_CMD, ATA_CMD_IDENTIFY);
    io_wait();

    let status = inb(base + ATA_REG_STATUS);
    if status == 0 { return None; } // drive does not exist

    if !ata_wait_bsy(base, 500_000) { return None; }

    // Check for ATAPI (LBA_MID/HI != 0 after IDENTIFY)
    let lba_mid = inb(base + ATA_REG_LBA_MID);
    let lba_hi  = inb(base + ATA_REG_LBA_HI);
    let mut is_atapi = false;

    if lba_mid != 0 || lba_hi != 0 {
        // Could be ATAPI: try IDENTIFY PACKET
        if (lba_mid == 0x14 && lba_hi == 0xEB) || (lba_mid == 0x69 && lba_hi == 0x96) {
            outb(base + ATA_REG_CMD, ATA_CMD_IDENTIFY_PACKET);
            io_wait();
            if !ata_wait_bsy(base, 500_000) { return None; }
            is_atapi = true;
        } else {
            return None;
        }
    }

    if !ata_wait_drq(base, 500_000) { return None; }

    // Read 256 words
    let mut buf = [0u16; 256];
    for i in 0..256 {
        buf[i] = inw(base + ATA_REG_DATA);
    }

    let mut d = DiskInfo::empty();
    d.present  = true;
    d.is_atapi = is_atapi;

    // Model: words 27-46, big-endian byte pairs
    for i in 0..20usize {
        let w = buf[27 + i];
        d.model[i*2]     = (w >> 8) as u8;
        d.model[i*2 + 1] = (w & 0xFF) as u8;
    }
    d.model[40] = 0;
    // Trim trailing spaces
    let mut end = 40usize;
    while end > 0 && (d.model[end-1] == b' ' || d.model[end-1] == 0) { end -= 1; }
    d.model[end] = 0;

    // Serial: words 10-19
    for i in 0..10usize {
        let w = buf[10 + i];
        d.serial[i*2]     = (w >> 8) as u8;
        d.serial[i*2 + 1] = (w & 0xFF) as u8;
    }
    d.serial[20] = 0;
    let mut end = 20usize;
    while end > 0 && (d.serial[end-1] == b' ' || d.serial[end-1] == 0) { end -= 1; }
    d.serial[end] = 0;

    // Size: LBA48 (words 100-103) if supported, else LBA28 (words 60-61)
    let support_lba48 = (buf[83] >> 10) & 1 != 0;
    if support_lba48 && !is_atapi {
        d.lba48 = true;
        let sectors = (buf[100] as u64)
            | ((buf[101] as u64) << 16)
            | ((buf[102] as u64) << 32)
            | ((buf[103] as u64) << 48);
        d.size_mb = sectors / 2048; // 512-byte sectors → MiB
    } else if !is_atapi {
        let sectors = (buf[60] as u64) | ((buf[61] as u64) << 16);
        d.size_mb = sectors / 2048;
    }

    Some(d)
}

pub struct Disks {
    pub drives: [DiskInfo; MAX_DISKS],
    pub count:  usize,
}

impl Disks {
    pub fn detect() -> Self {
        let mut disks = Disks {
            drives: [DiskInfo::empty(); MAX_DISKS],
            count: 0,
        };

        // (base, ctrl, bus_idx, drive_idx)
        let controllers: [(u16, u16, u8, u8); 4] = [
            (0x1F0, 0x3F6, 0, 0), // Primary Master
            (0x1F0, 0x3F6, 0, 1), // Primary Slave
            (0x170, 0x376, 1, 0), // Secondary Master
            (0x170, 0x376, 1, 1), // Secondary Slave
        ];

        for (i, &(base, ctrl, bus, drv)) in controllers.iter().enumerate() {
            if let Some(mut d) = unsafe { ata_identify(base, ctrl, drv) } {
                d.bus   = bus;
                d.drive = drv;
                disks.drives[disks.count] = d;
                disks.count += 1;
            }
            let _ = i;
        }

        disks
    }
}

// ── RAM Info (from E820 table written by stage2) ──────────────────────────────
pub struct RamInfo {
    pub usable_mb:  u64,   // Type 1 entries
    pub total_mb:   u64,   // All entries (incl. reserved/ACPI)
    pub entry_count: u16,
}

impl RamInfo {
    pub fn detect() -> Self {
        let mut info = RamInfo { usable_mb: 0, total_mb: 0, entry_count: 0 };
        unsafe {
            let count = core::ptr::read_volatile(0x9100 as *const u16);
            info.entry_count = count;
            let cap = count.min(128) as usize;
            for i in 0..cap {
                let p = (0x9102usize + i * 20) as *const u8;
                let len  = core::ptr::read_unaligned(p.add(8) as *const u64);
                let kind = core::ptr::read_unaligned(p.add(16) as *const u32);
                let mb = len / (1024 * 1024);
                // Only count actual physical RAM; skip MMIO/reserved (fixes 12GB false total)
                match kind {
                    1 => { info.usable_mb += mb; info.total_mb += mb; }
                    3 => { info.total_mb += mb; }  // ACPI reclaimable
                    _ => {}
                }
            }
        }
        info
    }
    /// Returns usable MB or a reasonable default
    pub fn usable_or_default(&self) -> u64 {
        if self.usable_mb == 0 { 64 } else { self.usable_mb }
    }
}

// ── Display Info (from VESA data written by stage2) ───────────────────────────
pub struct DisplayInfo {
    pub lfb_addr: u64,
    pub width:    u16,
    pub height:   u16,
    pub pitch:    u16,
    pub bpp:      u8,
}

impl DisplayInfo {
    pub fn detect() -> Self {
        unsafe {
            DisplayInfo {
                lfb_addr: core::ptr::read_volatile(0x9004 as *const u32) as u64,
                width:    core::ptr::read_volatile(0x9008 as *const u16),
                height:   core::ptr::read_volatile(0x900A as *const u16),
                pitch:    core::ptr::read_volatile(0x900C as *const u16),
                bpp:      core::ptr::read_volatile(0x900E as *const u8),
            }
        }
    }
    pub fn total_vram_kb(&self) -> u32 {
        let bytes = self.pitch as u32 * self.height as u32;
        bytes / 1024
    }
}

// ── Full hardware snapshot ────────────────────────────────────────────────────
pub struct HardwareInfo {
    pub cpu:     CpuInfo,
    pub ram:     RamInfo,
    pub disks:   Disks,
    pub display: DisplayInfo,
}

impl HardwareInfo {
    pub fn detect_all() -> Self {
        HardwareInfo {
            cpu:     CpuInfo::detect(),
            ram:     RamInfo::detect(),
            disks:   Disks::detect(),
            display: DisplayInfo::detect(),
        }
    }
}