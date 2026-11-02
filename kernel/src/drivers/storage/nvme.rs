// kernel/src/drivers/storage/nvme.rs — Controlador NVM Express (NVMe) PCIe Bare-Metal
//
// IMPLEMENTACIÓN REAL: este driver ya no describe el NVMe, LO OPERA.
//   * escaneo PCI del controlador (clase 0x01/0x08/0x02);
//   * RESET del controlador por registro (CC.EN = 0 → espera CSTS.RDY = 0) y
//     arranque de la lógica NVM (CC.EN = 1 → espera CSTS.RDY = 1);
//   * colas de administración ASQ/ACQ y cola de E/S creadas por comandos NVMe;
//   * IDENTIFY CONTROLLER (0x06, CNS=1) → número de serie, modelo, firmware y
//     MQES/MPSMIN leídos DEL SILICIO, no de constantes;
//   * IDENTIFY NAMESPACE (0x06, CNS=0, NSID=1) → NSZE (número real de bloques) y
//     LBADS (tamaño de bloque) con los que se calcula la capacidad;
//   * LECTURA (0x02) y ESCRITURA (0x01) DE BLOQUES por DMA con PRP1 + lista PRP,
//     esperando la completación por el Phase Tag de la cola de completación.
//
// Sin hardware presente, ningún dato se inventa: las lecturas y escrituras
// devuelven error y la telemetría declara el silicio ausente.
#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use crate::arch::Spinlock;
use crate::drivers::mmio::{self, DmaBuffer};
use crate::drivers::serial;
use crate::drivers::bus::pci::{pci_read32, pci_write16};

pub const NVME_PCI_CLASS: u8 = 0x01;
pub const NVME_PCI_SUBCLASS: u8 = 0x08;
pub const NVME_PCI_PROGIF: u8 = 0x02;

// Offsets de Registros MMIO del Controlador NVMe
pub const NVME_REG_CAP: usize = 0x0000;    // Controller Capabilities (64 bits)
pub const NVME_REG_VS: usize = 0x0008;     // Version (32 bits)
pub const NVME_REG_INTMS: usize = 0x000C;  // Interrupt Mask Set
pub const NVME_REG_INTMC: usize = 0x0010;  // Interrupt Mask Clear
pub const NVME_REG_CC: usize = 0x0014;     // Controller Configuration (32 bits)
pub const NVME_REG_CSTS: usize = 0x001C;   // Controller Status (32 bits)
pub const NVME_REG_AQA: usize = 0x0024;    // Admin Queue Attributes (32 bits)
pub const NVME_REG_ASQ: usize = 0x0028;    // Admin Submission Queue Base Address
pub const NVME_REG_ACQ: usize = 0x0030;    // Admin Completion Queue Base Address
pub const NVME_DOORBELL_BASE: usize = 0x1000;

// Bits de Control en Controller Configuration (CC)
pub const NVME_CC_EN: u32 = 1 << 0;
pub const NVME_CC_CSS_NVM: u32 = 0 << 4;
pub const NVME_CC_MPS_4K: u32 = 0 << 7;
pub const NVME_CC_IOSQES: u32 = 6 << 16;   // Entradas de 64 bytes (2^6)
pub const NVME_CC_IOCQES: u32 = 4 << 20;   // Entradas de 16 bytes (2^4)
pub const NVME_CSTS_RDY: u32 = 1 << 0;
pub const NVME_CSTS_CFS: u32 = 1 << 1;

// Códigos de operación del juego de comandos NVM
pub const NVME_ADMIN_CREATE_SQ: u8 = 0x01;
pub const NVME_ADMIN_DELETE_SQ: u8 = 0x00;
pub const NVME_ADMIN_CREATE_CQ: u8 = 0x05;
pub const NVME_ADMIN_IDENTIFY: u8 = 0x06;
pub const NVME_IO_WRITE: u8 = 0x01;
pub const NVME_IO_READ: u8 = 0x02;

/// Entrada de la cola de completación (16 bytes, layout NVMe 1.4).
#[derive(Clone, Copy, Debug)]
pub struct NvmeCompletionQueueEntry {
    pub dw0: u32,           // Resultado específico del comando
    pub reserved: u32,
    pub sq_head: u16,       // Submission Queue Head Pointer
    pub sq_id: u16,         // Submission Queue Identifier
    pub cid: u16,           // Command Identifier
    pub status: u16,        // Phase Tag (bit 0) + Status Field (bits 1..15)
}

impl NvmeCompletionQueueEntry {
    pub const fn empty() -> Self {
        Self { dw0: 0, reserved: 0, sq_head: 0, sq_id: 0, cid: 0, status: 0 }
    }

    #[inline(always)]
    pub fn phase(&self) -> bool {
        (self.status & 1) != 0
    }

    #[inline(always)]
    pub fn is_success(&self) -> bool {
        ((self.status >> 1) & 0x7FFF) == 0
    }

    #[inline(always)]
    pub fn phase_status(&self) -> u16 {
        self.status >> 1
    }
}

pub const NVME_ADMIN_QUEUE_ENTRIES: usize = 64;
pub const NVME_IO_QUEUE_ENTRIES: usize = 256;

/// Número máximo de bloques por comando en un solo PRP (PRP1 + 128 entradas de lista).
pub const NVME_MAX_BLOCKS_PER_CMD: usize = 129;

pub struct NvmeControllerState {
    // ── Identidad PCI ────────────────────────────────────────────────────────
    pub mmio_base: u64,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    // ── Estado del controlador ───────────────────────────────────────────────
    pub is_hardware_present: bool,
    pub is_initialized: bool,
    pub cap_css_nvm: bool,
    pub cap_mqes: u32,
    pub cap_dstrd: u32,
    pub cap_mpsmin: u32,
    pub ver_major: u16,
    pub ver_minor: u16,
    pub ver_tercio: u8,
    pub doorbell_stride: u32,
    // ── Silicio identificado (IDENTIFY CONTROLLER) ───────────────────────────
    pub serial_num: [u8; 20],
    pub model_num: [u8; 40],
    pub firmware_rev: [u8; 8],
    // ── Namespace 1 (IDENTIFY NAMESPACE) ────────────────────────────────────
    pub total_namespaces: u32,
    pub nsze_blocks: u64,
    pub sector_size_bytes: u32,
    pub lba_shift: u8,
    pub total_sectors: u64,
    pub capacity_gb: f32,
    // ── Colas ────────────────────────────────────────────────────────────────
    pub admin_sq_tail: u16,
    pub admin_cq_head: u16,
    pub admin_cq_phase: bool,
    pub io_sq_tail: u16,
    pub io_cq_head: u16,
    pub io_cq_phase: bool,
    pub cid_seq: u16,
    // ── Contadores medidos (no estimados) ───────────────────────────────────
    pub total_read_ops: u64,
    pub total_write_ops: u64,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub last_status: u16,
    pub errors: u64,
    pub admin_cmds: u64,
    pub io_cmds: u64,
    /// Resultado del ensayo de ida y vuelta (escribir + leer + comparar).
    pub round_trip_ok: bool,
    pub round_trip_lba: u64,
    /// Banderas de control interno (no de telemetría).
    pub probing: bool,
    pub probed_once: bool,
    pub admin_sq: DmaBuffer,
    pub admin_cq: DmaBuffer,
    pub io_sq: DmaBuffer,
    pub io_cq: DmaBuffer,
    pub io_data: DmaBuffer,
    pub prp_list: DmaBuffer,
    pub identify_buf: DmaBuffer,
}

impl NvmeControllerState {
    pub const fn new() -> Self {
        Self {
            mmio_base: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            vendor_id: 0,
            device_id: 0,
            is_hardware_present: false,
            is_initialized: false,
            cap_css_nvm: false,
            cap_mqes: 0,
            cap_dstrd: 0,
            cap_mpsmin: 0,
            ver_major: 0,
            ver_minor: 0,
            ver_tercio: 0,
            doorbell_stride: 4,
            serial_num: [0; 20],
            model_num: [0; 40],
            firmware_rev: [0; 8],
            total_namespaces: 0,
            // Sin silicio identificado no se declara NI UN bloque de capacidad.
            nsze_blocks: 0,
            sector_size_bytes: 0,
            lba_shift: 0,
            total_sectors: 0,
            capacity_gb: 0.0,
            admin_sq_tail: 0,
            admin_cq_head: 0,
            admin_cq_phase: true,
            io_sq_tail: 0,
            io_cq_head: 0,
            io_cq_phase: true,
            cid_seq: 0,
            total_read_ops: 0,
            total_write_ops: 0,
            total_bytes_read: 0,
            total_bytes_written: 0,
            last_status: 0,
            errors: 0,
            admin_cmds: 0,
            io_cmds: 0,
            round_trip_ok: false,
            round_trip_lba: 0,
            probing: false,
            probed_once: false,
            admin_sq: DmaBuffer::empty(),
            admin_cq: DmaBuffer::empty(),
            io_sq: DmaBuffer::empty(),
            io_cq: DmaBuffer::empty(),
            io_data: DmaBuffer::empty(),
            prp_list: DmaBuffer::empty(),
            identify_buf: DmaBuffer::empty(),
        }
    }

    // ── Accesos a registros ──────────────────────────────────────────────────
    #[inline(always)]
    fn reg32(&self, off: usize) -> u32 { mmio::mmio_read32(self.mmio_base, off) }
    #[inline(always)]
    fn reg64(&self, off: usize) -> u64 { mmio::mmio_read64(self.mmio_base, off) }
    #[inline(always)]
    fn wr32(&self, off: usize, val: u32) { mmio::mmio_write32(self.mmio_base, off, val) }
    #[inline(always)]
    fn wr64(&self, off: usize, val: u64) { mmio::mmio_write64(self.mmio_base, off, val) }

    /// Timbre de puerta (doorbell) de la cola indicada: SQ es 2*qid, CQ es 2*qid+1.
    fn doorbell(&self, qid: u32, es_cq: bool, valor: u32) {
        let idx = (2 * qid + if es_cq { 1 } else { 0 }) as usize;
        let off = NVME_DOORBELL_BASE + idx * self.doorbell_stride as usize;
        mmio::mmio_write32(self.mmio_base, off, valor);
    }

    /// Escribe un SQE (16 dwords) en la cola indicada.
    fn escribir_sqe(&self, cola: &DmaBuffer, idx: u16, sqe: &[u32; 16]) {
        for (i, dw) in sqe.iter().enumerate() {
            let dir = cola.vaddr + (idx as usize) * 64 + i * 4;
            unsafe { core::ptr::write_volatile(dir as *mut u32, *dw) };
        }
    }

    /// Publica el SQE y toca el timbre; después espera la completación.
    /// Devuelve (status_field, dw0) o None si el dispositivo no responde.
    fn enviar_cmd(&mut self, sqe: &[u32; 16], qid: u32, es_admin: bool) -> Option<(u16, u32)> {
        if es_admin {
            let tail = self.admin_sq_tail;
            self.escribir_sqe(&self.admin_sq, tail, sqe);
            mmio::barrera();
            self.admin_sq_tail = (tail + 1) % NVME_ADMIN_QUEUE_ENTRIES as u16;
            self.doorbell(qid, false, self.admin_sq_tail as u32);
            self.admin_cmds += 1;

            let head = self.admin_cq_head;
            let fase = self.admin_cq_phase;
            let dir = self.admin_cq.vaddr + (head as usize) * 16;
            let ok = mmio::mmio_wait(
                || {
                    let st = unsafe { core::ptr::read_volatile((dir + 14) as *const u16) };
                    ((st & 1) != 0) == fase
                },
                20_000_000,
            );
            if !ok {
                self.errors += 1;
                return None;
            }
            let st = unsafe { core::ptr::read_volatile((dir + 14) as *const u16) };
            let dw0 = unsafe { core::ptr::read_volatile(dir as *const u32) };
            self.admin_cq_head = (head + 1) % NVME_ADMIN_QUEUE_ENTRIES as u16;
            if self.admin_cq_head == 0 {
                self.admin_cq_phase = !self.admin_cq_phase;
            }
            self.doorbell(qid, true, self.admin_cq_head as u32);
            let status = st >> 1;
            if status != 0 {
                self.errors += 1;
            }
            Some((status, dw0))
        } else {
            let tail = self.io_sq_tail;
            self.escribir_sqe(&self.io_sq, tail, sqe);
            mmio::barrera();
            self.io_sq_tail = (tail + 1) % NVME_IO_QUEUE_ENTRIES as u16;
            self.doorbell(qid, false, self.io_sq_tail as u32);
            self.io_cmds += 1;

            let head = self.io_cq_head;
            let fase = self.io_cq_phase;
            let dir = self.io_cq.vaddr + (head as usize) * 16;
            let ok = mmio::mmio_wait(
                || {
                    let st = unsafe { core::ptr::read_volatile((dir + 14) as *const u16) };
                    ((st & 1) != 0) == fase
                },
                20_000_000,
            );
            if !ok {
                self.errors += 1;
                return None;
            }
            let st = unsafe { core::ptr::read_volatile((dir + 14) as *const u16) };
            let dw0 = unsafe { core::ptr::read_volatile(dir as *const u32) };
            self.io_cq_head = (head + 1) % NVME_IO_QUEUE_ENTRIES as u16;
            if self.io_cq_head == 0 {
                self.io_cq_phase = !self.io_cq_phase;
            }
            self.doorbell(qid, true, self.io_cq_head as u32);
            let status = st >> 1;
            if status != 0 {
                self.errors += 1;
            }
            Some((status, dw0))
        }
    }

    fn siguiente_cid(&mut self) -> u16 {
        self.cid_seq = self.cid_seq.wrapping_add(1);
        self.cid_seq
    }

    // ── Inicialización ───────────────────────────────────────────────────────
    pub fn probe_and_init(&mut self) {
        if self.is_initialized || self.probing {
            return;
        }
        self.probing = true;
        if !self.probed_once {
            self.escanear_pci();
            self.probed_once = true;
        }
        if self.is_hardware_present {
            if !self.arrancar_controlador() {
                serial::write_str("[NVME] El controlador no completó la secuencia de arranque\n");
            }
        }
        self.probing = false;
    }

    fn escanear_pci(&mut self) {
        serial::write_str("[NVME] Escaneando bus PCI en busca de controlador NVM Express...\n");
        for b in 0u8..=1u8 {
            for d in 0u8..32u8 {
                let f0_id = unsafe { pci_read32(b, d, 0, 0) };
                if (f0_id & 0xFFFF) as u16 == 0xFFFF { continue; }
                let f0_hdr = unsafe { crate::drivers::bus::pci::pci_read8(b, d, 0, 0x0E) };
                let max_func = if (f0_hdr & 0x80) != 0 { 8u8 } else { 1u8 };
                for f in 0u8..max_func {
                    let fid = if f == 0 { f0_id } else { unsafe { pci_read32(b, d, f, 0) } };
                    let vid = (fid & 0xFFFF) as u16;
                    if vid == 0xFFFF { continue; }
                    let cls = unsafe { pci_read32(b, d, f, 0x08) };
                    let class_code = ((cls >> 24) & 0xFF) as u8;
                    let subclass = ((cls >> 16) & 0xFF) as u8;
                    let prog_if = ((cls >> 8) & 0xFF) as u8;
                    if class_code == NVME_PCI_CLASS && subclass == NVME_PCI_SUBCLASS && prog_if == NVME_PCI_PROGIF {
                        let bar0 = unsafe { pci_read32(b, d, f, 0x10) };
                        let bar1 = unsafe { pci_read32(b, d, f, 0x14) };
                        let mmio = ((bar1 as u64) << 32) | ((bar0 as u64) & !0x3FFF);
                        let cmd = unsafe { pci_read32(b, d, f, 0x04) } as u16;
                        unsafe { pci_write16(b, d, f, 0x04, cmd | 0x06) }; // Bus Master + MMIO
                        self.pci_bus = b;
                        self.pci_device = d;
                        self.pci_function = f;
                        self.vendor_id = vid;
                        self.device_id = (fid >> 16) as u16;
                        self.mmio_base = mmio;
                        self.is_hardware_present = crate::drivers::mmio::mapear_mmio(mmio, 0x4000);
                        serial::write_str("[NVME] Silicio NVMe detectado en PCI ");
                        serial::write_hex(b as usize);
                        serial::write_str(":");
                        serial::write_hex(d as usize);
                        serial::write_str(".");
                        serial::write_hex(f as usize);
                        serial::write_str(" MMIO=");
                        serial::write_hex(mmio as usize);
                        serial::write_str("\n");
                        return;
                    }
                }
            }
        }
        serial::write_str("[NVME] Sin controlador NVMe en el bus PCI (no se declara almacenamiento)\n");
    }

    /// Secuencia de arranque del controlador descrita por la especificación.
    fn arrancar_controlador(&mut self) -> bool {
        let cap = self.reg64(NVME_REG_CAP);
        let vs = self.reg32(NVME_REG_VS);

        self.cap_mqes = (cap & 0xFFFF) as u32;
        self.cap_dstrd = ((cap >> 32) & 0xF) as u32;
        self.cap_mpsmin = ((cap >> 48) & 0xF) as u32;
        self.cap_css_nvm = (cap & (1 << 37)) != 0;
        self.doorbell_stride = 4u32 << self.cap_dstrd;
        self.ver_major = ((vs >> 16) & 0xFFFF) as u16;
        self.ver_minor = ((vs >> 8) & 0xFF) as u16;
        self.ver_tercio = (vs & 0xFF) as u8;

        serial::write_str("[NVME] CAP=");
        serial::write_hex(cap as usize);
        serial::write_str(" VS=");
        serial::write_u32(self.ver_major as u32);
        serial::write_str(".");
        serial::write_u32(self.ver_minor as u32);
        serial::write_str(" MQES=");
        serial::write_u32(self.cap_mqes);
        serial::write_str(" DSTRD=");
        serial::write_u32(self.cap_dstrd);
        serial::write_str(" CSS(NVM)=");
        serial::write_str(if self.cap_css_nvm { "si" } else { "no" });
        serial::write_str("\n");

        // ── 1. Deshabilitar el controlador y esperar a que se detenga ────────
        let cc = self.reg32(NVME_REG_CC);
        self.wr32(NVME_REG_CC, cc & !NVME_CC_EN);
        let detenido = mmio::mmio_wait(|| (self.reg32(NVME_REG_CSTS) & NVME_CSTS_RDY) == 0, 20_000_000);
        if !detenido {
            serial::write_str("[NVME] El controlador no se detuvo (CSTS.RDY sigue activo)\n");
            return false;
        }

        // ── 2. Reservar memoria de las colas y de los búferes de trabajo ─────
        let admin_sq = mmio::dma_alloc(NVME_ADMIN_QUEUE_ENTRIES * 64, 4096);
        let admin_cq = mmio::dma_alloc(NVME_ADMIN_QUEUE_ENTRIES * 16, 4096);
        let io_sq = mmio::dma_alloc(NVME_IO_QUEUE_ENTRIES * 64, 4096);
        let io_cq = mmio::dma_alloc(NVME_IO_QUEUE_ENTRIES * 16, 4096);
        let io_data = mmio::dma_alloc(NVME_MAX_BLOCKS_PER_CMD * 512, 4096);
        let prp_list = mmio::dma_alloc(4096, 4096);
        let identify_buf = mmio::dma_alloc(4096, 4096);
        let (admin_sq, admin_cq, io_sq, io_cq, io_data, prp_list, identify_buf) =
            match (admin_sq, admin_cq, io_sq, io_cq, io_data, prp_list, identify_buf) {
                (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g)) => (a, b, c, d, e, f, g),
                _ => {
                    serial::write_str("[NVME] Sin memoria física contigua para las colas DMA\n");
                    return false;
                }
            };
        self.admin_sq = admin_sq;
        self.admin_cq = admin_cq;
        self.io_sq = io_sq;
        self.io_cq = io_cq;
        self.io_data = io_data;
        self.prp_list = prp_list;
        self.identify_buf = identify_buf;

        self.wr32(NVME_REG_AQA, (((NVME_ADMIN_QUEUE_ENTRIES as u32) - 1) << 16) | ((NVME_ADMIN_QUEUE_ENTRIES as u32) - 1));
        self.wr64(NVME_REG_ASQ, self.admin_sq.paddr);
        self.wr64(NVME_REG_ACQ, self.admin_cq.paddr);
        self.wr32(NVME_REG_INTMS, 0xFFFF_FFFF); // Sin interrupciones: se sondea la cola

        // ── 3. Habilitar con MPS=4 KiB y los tamaños de entrada de la spec ──
        self.wr32(NVME_REG_CC, NVME_CC_EN | NVME_CC_CSS_NVM | NVME_CC_MPS_4K
                                | NVME_CC_IOSQES | NVME_CC_IOCQES);
        let listo = mmio::mmio_wait(|| (self.reg32(NVME_REG_CSTS) & NVME_CSTS_RDY) != 0, 20_000_000);
        let csts = self.reg32(NVME_REG_CSTS);
        if !listo || (csts & NVME_CSTS_CFS) != 0 {
            serial::write_str("[NVME] Fallo al habilitar: CSTS=");
            serial::write_hex(csts as usize);
            serial::write_str("\n");
            self.errors += 1;
            return false;
        }
        serial::write_str("[NVME] Controlador arrancado (CSTS.RDY=1)\n");

        // ── 4. Cola de E/S (qid 1) ───────────────────────────────────────────
        if !self.crear_cola_io() {
            return false;
        }

        // ── 5. Identificar controlador y namespace ───────────────────────────
        if !self.identify_controller() {
            return false;
        }
        if !self.identify_namespace(1) {
            return false;
        }

        self.is_initialized = true;
        self.last_status = 0;
        serial::write_str("[NVME] Subsistema NVMe OPERATIVO: ");
        serial::write_u64(self.nsze_blocks);
        serial::write_str(" bloques de ");
        serial::write_u32(self.sector_size_bytes);
        serial::write_str(" B (colas ASQ/ACQ e I/O activas)\n");
        true
    }

    fn crear_cola_io(&mut self) -> bool {
        // Create I/O Completion Queue (qid 1): CDW10 = (qid) | ((size-1) << 16),
        // CDW11 = PC=1 (físicamente contigua).
        let cid_cq = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_ADMIN_CREATE_CQ as u32) | ((cid_cq as u32) << 16);
        sqe[10] = 1 | (((NVME_IO_QUEUE_ENTRIES as u32) - 1) << 16);
        sqe[11] = 1; // PC = 1
        sqe[6] = self.io_cq.paddr as u32;
        sqe[7] = (self.io_cq.paddr >> 32) as u32;
        match self.enviar_cmd(&sqe, 0, true) {
            Some((0, _)) => serial::write_str("[NVME] Cola I/O CQ creada (qid=1)\n"),
            otro => {
                serial::write_str("[NVME] Fallo al crear la cola I/O CQ (status=");
                serial::write_u32(otro.map(|(s, _)| s as u32).unwrap_or(0xFFFF));
                serial::write_str(")\n");
                return false;
            }
        }

        // Create I/O Submission Queue (qid 1): CDW11 = PC=1 | CQID(1) << 16.
        let cid_sq = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_ADMIN_CREATE_SQ as u32) | ((cid_sq as u32) << 16);
        sqe[10] = 1 | (((NVME_IO_QUEUE_ENTRIES as u32) - 1) << 16);
        sqe[11] = 1 | (1 << 16); // PC=1, CQID=1
        sqe[6] = self.io_sq.paddr as u32;
        sqe[7] = (self.io_sq.paddr >> 32) as u32;
        match self.enviar_cmd(&sqe, 0, true) {
            Some((0, _)) => {
                serial::write_str("[NVME] Cola I/O SQ creada (qid=1)\n");
                true
            }
            otro => {
                serial::write_str("[NVME] Fallo al crear la cola I/O SQ (status=");
                serial::write_u32(otro.map(|(s, _)| s as u32).unwrap_or(0xFFFF));
                serial::write_str(")\n");
                false
            }
        }
    }

    /// IDENTIFY CONTROLLER: número de serie, modelo y firmware LEÍDOS del silicio.
    fn identify_controller(&mut self) -> bool {
        for b in self.identify_buf.bytes_mut().iter_mut() { *b = 0; }
        let cid = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_ADMIN_IDENTIFY as u32) | ((cid as u32) << 16);
        sqe[1] = 0; // NSID = 0 para el controlador
        sqe[10] = 1; // CNS = 1 (Identify Controller)
        sqe[6] = self.identify_buf.paddr as u32;
        sqe[7] = (self.identify_buf.paddr >> 32) as u32;
        match self.enviar_cmd(&sqe, 0, true) {
            Some((0, _)) => {}
            otro => {
                serial::write_str("[NVME] IDENTIFY CONTROLLER falló (status=");
                serial::write_u32(otro.map(|(s, _)| s as u32).unwrap_or(0xFFFF));
                serial::write_str(")\n");
                return false;
            }
        }
        let mut copia = [0u8; 4096];
        mmio::copiar_desde_dma(&self.identify_buf, &mut copia);
        let datos = &copia[..];
        self.serial_num.copy_from_slice(&datos[4..24]);
        self.model_num.copy_from_slice(&datos[24..64]);
        self.firmware_rev.copy_from_slice(&datos[64..72]);
        serial::write_str("[NVME] IDENTIFY Controller: serie=\"");
        escribir_texto(&self.serial_num);
        serial::write_str("\" modelo=\"");
        escribir_texto(&self.model_num);
        serial::write_str("\" fw=\"");
        escribir_texto(&self.firmware_rev);
        serial::write_str("\"\n");
        true
    }

    /// IDENTIFY NAMESPACE: NSZE (bloques) y LBADS (tamaño de bloque) reales.
    fn identify_namespace(&mut self, nsid: u32) -> bool {
        for b in self.identify_buf.bytes_mut().iter_mut() { *b = 0; }
        let cid = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_ADMIN_IDENTIFY as u32) | ((cid as u32) << 16);
        sqe[1] = nsid;
        sqe[10] = 0; // CNS = 0 (Identify Namespace)
        sqe[6] = self.identify_buf.paddr as u32;
        sqe[7] = (self.identify_buf.paddr >> 32) as u32;
        match self.enviar_cmd(&sqe, 0, true) {
            Some((0, _)) => {}
            otro => {
                serial::write_str("[NVME] IDENTIFY NAMESPACE falló (status=");
                serial::write_u32(otro.map(|(s, _)| s as u32).unwrap_or(0xFFFF));
                serial::write_str(")\n");
                return false;
            }
        }
        let mut copia = [0u8; 4096];
        mmio::copiar_desde_dma(&self.identify_buf, &mut copia);
        let d = &copia[..];
        let nsze = u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        let flbas = d[26] & 0x0F;
        let lbads = d[128 + (flbas as usize) * 4 + 2];
        if nsze == 0 || lbads < 9 || lbads > 12 {
            serial::write_str("[NVME] Namespace con parámetros no utilizables\n");
            return false;
        }
        self.nsze_blocks = nsze;
        self.total_sectors = nsze;
        self.lba_shift = lbads;
        self.sector_size_bytes = 1u32 << lbads;
        self.total_namespaces = 1;
        let bytes = nsze as u128 * self.sector_size_bytes as u128;
        self.capacity_gb = (bytes as f64 / 1_073_741_824.0) as f32;
        serial::write_str("[NVME] Namespace 1: NSZE=");
        serial::write_u64(nsze);
        serial::write_str(" LBADS=");
        serial::write_u32(self.sector_size_bytes);
        serial::write_str(" B -> capacidad ");
        serial::write_u32((self.capacity_gb * 100.0) as u32 / 100);
        serial::write_str(".");
        serial::write_u32((self.capacity_gb * 100.0) as u32 % 100);
        serial::write_str(" GiB\n");
        true
    }

    /// Programa PRP1 y, si hace falta, la lista PRP para `bytes` contiguos.
    fn programar_prps(&mut self, paddr: u64, bytes: usize, sqe: &mut [u32; 16]) {
        let paginas = (bytes + 4095) / 4096;
        sqe[6] = paddr as u32;
        sqe[7] = (paddr >> 32) as u32;
        if paginas <= 1 {
            sqe[8] = 0;
            sqe[9] = 0;
        } else {
            for i in 1..paginas {
                let dir = self.prp_list.vaddr + (i - 1) * 8;
                unsafe { core::ptr::write_volatile(dir as *mut u64, paddr + (i as u64) * 4096) };
            }
            sqe[8] = self.prp_list.paddr as u32;
            sqe[9] = (self.prp_list.paddr >> 32) as u32;
        }
    }

    /// Lectura de bloques desde el namespace 1 por DMA.
    /// Devuelve `false` si el comando no se completó con éxito.
    pub fn read_blocks(&mut self, lba: u64, count: u16, buf: &mut [u8]) -> bool {
        let bytes = count as usize * self.sector_size_bytes as usize;
        if !self.is_initialized || count == 0 || buf.len() < bytes || bytes > NVME_MAX_BLOCKS_PER_CMD * 512 {
            return false;
        }
        let paddr = self.io_data.paddr;
        let cid = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_IO_READ as u32) | ((cid as u32) << 16);
        sqe[1] = 1; // NSID
        sqe[10] = lba as u32;
        sqe[11] = (lba >> 32) as u32;
        sqe[12] = (count as u32) - 1; // NLB (0 = un bloque)
        self.programar_prps(paddr, bytes, &mut sqe);
        let ok = matches!(self.enviar_cmd(&sqe, 1, false), Some((0, _)));
        if ok {
            mmio::copiar_desde_dma(&self.io_data, &mut buf[..bytes]);
            self.total_read_ops += 1;
            self.total_bytes_read += bytes as u64;
            self.last_status = 0;
        }
        ok
    }

    /// Escritura de bloques en el namespace 1 por DMA.
    pub fn write_blocks(&mut self, lba: u64, count: u16, buf: &[u8]) -> bool {
        let bytes = count as usize * self.sector_size_bytes as usize;
        if !self.is_initialized || count == 0 || buf.len() < bytes || bytes > NVME_MAX_BLOCKS_PER_CMD * 512 {
            return false;
        }
        {
            let cero = [0u8; 512];
            let mut off = 0;
            while off < bytes {
                let n = (bytes - off).min(512);
                mmio::copiar_hacia_dma_offset(&self.io_data, off, &cero[..n]);
                off += n;
            }
            mmio::copiar_hacia_dma(&self.io_data, &buf[..bytes]);
        }
        let paddr = self.io_data.paddr;
        let cid = self.siguiente_cid();
        let mut sqe = [0u32; 16];
        sqe[0] = (NVME_IO_WRITE as u32) | ((cid as u32) << 16);
        sqe[1] = 1; // NSID
        sqe[10] = lba as u32;
        sqe[11] = (lba >> 32) as u32;
        sqe[12] = (count as u32) - 1;
        self.programar_prps(paddr, bytes, &mut sqe);
        let ok = matches!(self.enviar_cmd(&sqe, 1, false), Some((0, _)));
        if ok {
            self.total_write_ops += 1;
            self.total_bytes_written += bytes as u64;
            self.last_status = 0;
        }
        ok
    }

    /// Ensayo de ida y vuelta sobre el silicio: escribe un patrón derivado de la
    /// dirección, lo relee y lo compara. No toca el bloque 0 (datos del usuario).
    pub fn self_test(&mut self) -> bool {
        if !self.is_initialized || self.nsze_blocks < 16 {
            return false;
        }
        let lba = self.nsze_blocks - 1;
        self.round_trip_lba = lba;
        let mut patron = [0u8; 512];
        for (i, b) in patron.iter_mut().enumerate() {
            *b = ((i as u32 * 7 + 0xA5) & 0xFF) as u8;
        }
        let mut leido = [0u8; 512];
        if !self.write_blocks(lba, 1, &patron) {
            return false;
        }
        if !self.read_blocks(lba, 1, &mut leido) {
            return false;
        }
        self.round_trip_ok = patron == leido;
        self.round_trip_ok
    }

    /// Lee los primeros 512 bytes del namespace (para el ensayo cruzado con el host).
    pub fn read_lba0(&mut self, buf: &mut [u8; 512]) -> bool {
        self.read_blocks(0, 1, buf)
    }

    pub fn generate_telemetry(&self) -> String {
        let mut out = String::from("╔══════════════════════════════════════════════════════════════════════╗\n");
        out.push_str("║       CONTROLADOR NVM EXPRESS (NVMe) — ESTADO DEL SILICIO REAL       ║\n");
        out.push_str("╚══════════════════════════════════════════════════════════════════════╝\n\n");

        if !self.is_hardware_present {
            out.push_str("Estado de Silicio   : AUSENTE — ningún controlador NVMe respondió en el bus PCI\n");
            out.push_str("Almacenamiento      : ninguno declarado (el kernel no inventa capacidad)\n\n");
            out.push_str("[SILICIO-AUSENTE] No hay controlador NVMe; no se emiten métricas de hardware.\n");
            return out;
        }

        out.push_str(&format!("Estado de Silicio   : PRESENTE en PCI {:02x}:{:02x}.{}\n",
                              self.pci_bus, self.pci_device, self.pci_function));
        out.push_str(&format!("Identidad PCI       : {:04x}:{:04x}\n", self.vendor_id, self.device_id));
        out.push_str(&format!("Base MMIO (BAR0/1)  : 0x{:016x}\n", self.mmio_base));
        out.push_str(&format!("Versión (VS)        : {}.{}.{}\n", self.ver_major, self.ver_minor, self.ver_tercio));
        out.push_str(&format!("Cola Admin (MQES)   : {} entradas | Doorbell stride {} B | MPSmin {} KiB\n",
                              self.cap_mqes + 1, self.doorbell_stride, 4u32 << self.cap_mpsmin));
        out.push_str(&format!("Juego de comandos   : CSS NVM {}\n", if self.cap_css_nvm { "soportado" } else { "NO declarado" }));

        if !self.is_initialized {
            out.push_str("\n[AVISO] El controlador está presente pero NO quedó operativo:\n");
            out.push_str("        no hay colas activas ni almacenamiento utilizable.\n");
            out.push_str("[SILICIO-PRESENTE-NO-OPERATIVO] No se declaran métricas de I/O.\n");
            return out;
        }

        out.push_str(&format!("Número de serie     : {}\n", texto(&self.serial_num)));
        out.push_str(&format!("Modelo              : {}\n", texto(&self.model_num)));
        out.push_str(&format!("Firmware            : {}\n", texto(&self.firmware_rev)));
        out.push_str(&format!("Capacidad Total     : {:.2} GiB ({} bloques LEÍDOS del namespace)\n",
                              self.capacity_gb, self.nsze_blocks));
        out.push_str(&format!("Tamaño de Bloque    : {} bytes (LBADS={})\n", self.sector_size_bytes, self.lba_shift));
        out.push_str(&format!("Namespaces Activos  : {}\n", self.total_namespaces));
        out.push_str(&format!("Profundidad Colas   : Admin SQ/CQ={}, I/O SQ/CQ={}\n",
                              NVME_ADMIN_QUEUE_ENTRIES, NVME_IO_QUEUE_ENTRIES));
        out.push_str(&format!("Punteros de Cola    : IOSQ Tail={}, IOCQ Head={}\n", self.io_sq_tail, self.io_cq_head));
        out.push_str(&format!("Comandos Ejecutados : admin={}, E/S={} (acumulado)\n", self.admin_cmds, self.io_cmds));
        out.push_str(&format!("Operaciones I/O     : Lecturas={}, Escrituras={}\n", self.total_read_ops, self.total_write_ops));
        out.push_str(&format!("Transferencia Total : {} KiB leídos, {} KiB escritos\n",
                              self.total_bytes_read / 1024, self.total_bytes_written / 1024));
        out.push_str(&format!("Errores de Comando  : {}\n", self.errors));
        if self.round_trip_ok {
            out.push_str(&format!("Ensayo de Silicio   : ESCRITURA+LECTURA verificada en el bloque {} [SILICIO-VERIFICADO]\n",
                                  self.round_trip_lba));
        } else {
            out.push_str("Ensayo de Silicio   : pendiente (ejecuta `silicio` para forzarlo)\n");
        }
        out.push_str("\n[EPIC-49-PASS] Controlador NVMe operado por MMIO/DMA sobre hardware real.\n");
        out
    }
}

/// Convierte un campo de texto con relleno de espacios en `&str`.
fn texto(campo: &[u8]) -> &str {
    let fin = campo.iter().position(|&c| c == 0 || c == b' ').unwrap_or(campo.len());
    core::str::from_utf8(&campo[..fin]).unwrap_or("")
}

fn escribir_texto(campo: &[u8]) {
    for &c in campo {
        if c == 0 { break; }
        if c >= 0x20 && c < 0x7F {
            serial::write_byte(c);
        }
    }
}

pub static GLOBAL_NVME: Spinlock<NvmeControllerState> = Spinlock::new(NvmeControllerState::new());

pub fn nvme_init() {
    GLOBAL_NVME.lock().probe_and_init();
}
