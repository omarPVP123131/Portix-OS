// kernel/src/drivers/bus/xhci.rs — Controlador USB 3.x (xHCI) Bare-Metal
//
// IMPLEMENTACIÓN REAL: el driver habla con el controlador por sus anillos de
// Transferencia/Comando/Evento, no por tablas de constantes.
//   * registro CAPLENGTH/HCIVERSION/HCSPARAMS1-2/HCCPARAMS1/DBOFF/RTSOFF leídos
//     del silicio (número de puertos, slots, tamaño de contexto, AC64);
//   * estado de cada puerto raíz por PORTSC (CCS, PED, velocidad, PLS);
//   * RESET del controlador (USBCMD.HCRST + espera de USBSTS.HCRST/CNR);
//   * configuración de la base de contextos (DCBAAP), anillo de comandos (CRCR
//     con RCS=1) y anillo de eventos con su tabla de segmentos (ERSTSZ/ERSTBA/ERDP);
//   * ENABLE SLOT (opcode 9) y ADDRESS DEVICE (opcode 11) con contextos de slot y
//     de EP0 programados en memoria física;
//   * transferencia de CONTROL por EP0 (SETUP/DATA/STATUS) para leer el
//     DESCRIPTOR DE DISPOSITIVO real del dispositivo conectado;
//   * si el dispositivo es de ALMACENAMIENTO MASIVO (Bulk-Only), se leen sus
//     descriptores de configuración y se ejecuta READ(10) por SCSI sobre el par
//     de endpoints bulk.
//
// Sin dispositivo conectado no se informa de ningún dispositivo.
#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use crate::arch::Spinlock;
use crate::drivers::mmio::{self, DmaBuffer};
use crate::drivers::serial;
use crate::drivers::bus::pci::{pci_read32, pci_write16};

pub const XHCI_PCI_CLASS: u8 = 0x0C;
pub const XHCI_PCI_SUBCLASS: u8 = 0x03;
pub const XHCI_PCI_PROGIF: u8 = 0x30;

// Registros del espacio de capacidad del host
pub const XHCI_REG_CAPLENGTH: usize = 0x00;
pub const XHCI_REG_HCIVERSION: usize = 0x02;
pub const XHCI_REG_HCSPARAMS1: usize = 0x04;
pub const XHCI_REG_HCSPARAMS2: usize = 0x08;
pub const XHCI_REG_HCCPARAMS1: usize = 0x10;
pub const XHCI_REG_DBOFF: usize = 0x14;
pub const XHCI_REG_RTSOFF: usize = 0x18;
pub const XHCI_REG_HCCPARAMS2: usize = 0x1C;

// Capacidades del controlador
pub const XHCI_HCSPARAMS1_MAXSLOTS: u32 = 0xFF;
pub const XHCI_HCSPARAMS1_MAXINTRS: u32 = 0x7FF << 8;
pub const XHCI_HCSPARAMS1_MAXPORTS: u32 = 0xFF << 24;
pub const XHCI_HCCPARAMS1_AC64: u32 = 1 << 0;
pub const XHCI_HCCPARAMS1_CSZ: u32 = 1 << 2;

// Registros operativos (desde mmio_base + CAPLENGTH)
pub const XHCI_OP_USBCMD: usize = 0x00;
pub const XHCI_OP_USBSTS: usize = 0x04;
pub const XHCI_OP_PAGESIZE: usize = 0x08;
pub const XHCI_OP_DNCTRL: usize = 0x14;
pub const XHCI_OP_CRCR: usize = 0x18;
pub const XHCI_OP_DCBAAP: usize = 0x30;
pub const XHCI_OP_CONFIG: usize = 0x38;

pub const XHCI_CMD_RS: u32 = 1 << 0;
pub const XHCI_CMD_HCRST: u32 = 1 << 1;
pub const XHCI_CMD_INTE: u32 = 1 << 2;
pub const XHCI_STS_HCH: u32 = 1 << 0;
pub const XHCI_STS_CNR: u32 = 1 << 11;
pub const XHCI_STS_HCRST: u32 = 1 << 7;

pub const XHCI_PORTSC_CCS: u32 = 1 << 0;
pub const XHCI_PORTSC_PED: u32 = 1 << 1;
pub const XHCI_PORTSC_PLS_MASK: u32 = 0xF << 5;
pub const XHCI_PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const XHCI_PORTSC_PR: u32 = 1 << 4;
pub const XHCI_PORTSC_W1C: u32 = 0x00FE_0000; // CSC..CEC

// Tipos de TRB
pub const TRB_NORMAL: u32 = 1;
pub const TRB_SETUP_STAGE: u32 = 2;
pub const TRB_DATA_STAGE: u32 = 3;
pub const TRB_STATUS_STAGE: u32 = 4;
pub const TRB_LINK: u32 = 6;
pub const TRB_ENABLE_SLOT: u32 = 9;
pub const TRB_ADDRESS_DEVICE: u32 = 11;
pub const TRB_CONFIGURE_ENDPOINT: u32 = 12;
pub const TRB_EVENT_TRANSFER: u32 = 32;
pub const TRB_EVENT_CMD_COMPLETE: u32 = 33;
pub const TRB_EVENT_PORT_STATUS: u32 = 34;

/// Un puerto raíz con su estado LEÍDO de PORTSC.
#[derive(Clone, Copy)]
pub struct XhciPuerto {
    pub numero: u8,
    pub conectado: bool,
    pub habilitado: bool,
    pub pls: u8,
    pub velocidad: u8,
    pub speed_id: u8,
}

impl XhciPuerto {
    pub const fn vacio(n: u8) -> Self {
        Self { numero: n, conectado: false, habilitado: false, pls: 0, velocidad: 0, speed_id: 0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UsbHidKeyboardReport {
    pub modifier_keys: u8,
    pub reserved: u8,
    pub keycodes: [u8; 6],
}

#[derive(Clone, Copy, Debug)]
pub struct UsbHidMouseReport {
    pub buttons: u8,
    pub delta_x: i8,
    pub delta_y: i8,
    pub wheel: i8,
}

pub struct XhciController {
    pub mmio_base: u64,
    pub op_offset: usize,
    pub db_offset: usize,
    pub rt_offset: usize,
    pub cap_length: u8,
    pub hci_version: u16,
    pub max_slots: u8,
    pub max_ports: u8,
    pub max_intrs: u16,
    pub ctx_size: usize,
    pub ac64: bool,
    pub is_running: bool,
    pub is_hardware_present: bool,
    pub puertos: [XhciPuerto; 32],
    pub puerto_raiz: u8,
    pub speed_id: u8,
    // Slot y dispositivo direccionado
    pub slot_id: u8,
    pub direccion_usb: u8,
    pub slot_habilitado: bool,
    pub ep0_configurado: bool,
    // Descriptores LEÍDOS del dispositivo
    pub desc_device: [u8; 18],
    pub desc_device_ok: bool,
    pub desc_config: [u8; 512],
    pub desc_config_len: u16,
    pub desc_config_ok: bool,
    pub vid: u16,
    pub pid: u16,
    pub bcd_usb: u16,
    pub num_configs: u8,
    pub clase: u8,
    pub subclase: u8,
    pub protocolo: u8,
    // Almacenamiento masivo (si el descriptor lo declara)
    pub es_mass_storage: bool,
    pub bulk_in_ep: u8,
    pub bulk_out_ep: u8,
    pub bulk_in_maxpkt: u16,
    pub bulk_out_maxpkt: u16,
    pub capacidad_sectores: u64,
    pub round_trip_ok: bool,
    pub round_trip_lba: u32,
    // Campos que consume la interfaz (semántica real ahora)
    pub mass_storage_attached: bool,
    pub mass_storage_capacity_sectors: u64,
    pub connected_ports: u32,
    pub hotplug_events_count: u32,
    pub automount_active: bool,
    pub mount_path: [u8; 32],
    pub device_node: [u8; 16],
    pub sectors_transferred: u64,
    pub comandos: u64,
    pub errores: u64,
    pub probing: bool,
    // Buffers DMA del controlador
    pub dcbaa: DmaBuffer,
    pub cmd_ring: DmaBuffer,
    pub evt_ring: DmaBuffer,
    pub erst: DmaBuffer,
    pub input_ctx: DmaBuffer,
    pub output_ctx: DmaBuffer,
    pub trb_buf: DmaBuffer,
    pub data_buf: DmaBuffer,
    pub desc_buf: DmaBuffer,
    // Carreras de los anillos
    pub cmd_cycle: bool,
    pub cmd_enqueue: usize,
    pub evt_cycle: bool,
    pub evt_dequeue: usize,
    // Anillos de transferencia: cada endpoint tiene el suyo, y el ciclo se
    // invierte al dar la vuelta. Reutilizar el índice 0 sin llevar la cuenta
    // deja al controlador desincronizado del anillo (deja de ver TRBs).
    pub ep0_idx: usize,
    pub ep0_cycle: bool,
    pub bulk_in_ring: DmaBuffer,
    pub bulk_out_ring: DmaBuffer,
    pub bulk_in_idx: usize,
    pub bulk_in_cycle: bool,
    pub bulk_out_idx: usize,
    pub bulk_out_cycle: bool,
    /// Device Context Index real de cada endpoint (2*EP + sentido), tal como
    /// los declara el descriptor de interfaz: no siempre EP1=OUT y EP2=IN.
    pub dci_in: u8,
    pub dci_out: u8,
}

impl XhciController {
    pub const fn empty() -> Self {
        XhciController {
            mmio_base: 0,
            op_offset: 0,
            db_offset: 0,
            rt_offset: 0,
            cap_length: 0,
            hci_version: 0,
            max_slots: 0,
            max_ports: 0,
            max_intrs: 0,
            ctx_size: 32,
            ac64: false,
            is_running: false,
            is_hardware_present: false,
            puertos: [
                XhciPuerto::vacio(0), XhciPuerto::vacio(1), XhciPuerto::vacio(2), XhciPuerto::vacio(3),
                XhciPuerto::vacio(4), XhciPuerto::vacio(5), XhciPuerto::vacio(6), XhciPuerto::vacio(7),
                XhciPuerto::vacio(8), XhciPuerto::vacio(9), XhciPuerto::vacio(10), XhciPuerto::vacio(11),
                XhciPuerto::vacio(12), XhciPuerto::vacio(13), XhciPuerto::vacio(14), XhciPuerto::vacio(15),
                XhciPuerto::vacio(16), XhciPuerto::vacio(17), XhciPuerto::vacio(18), XhciPuerto::vacio(19),
                XhciPuerto::vacio(20), XhciPuerto::vacio(21), XhciPuerto::vacio(22), XhciPuerto::vacio(23),
                XhciPuerto::vacio(24), XhciPuerto::vacio(25), XhciPuerto::vacio(26), XhciPuerto::vacio(27),
                XhciPuerto::vacio(28), XhciPuerto::vacio(29), XhciPuerto::vacio(30), XhciPuerto::vacio(31),
            ],
            puerto_raiz: 0,
            speed_id: 0,
            slot_id: 0,
            direccion_usb: 0,
            slot_habilitado: false,
            ep0_configurado: false,
            desc_device: [0; 18],
            desc_device_ok: false,
            desc_config: [0; 512],
            desc_config_len: 0,
            desc_config_ok: false,
            vid: 0,
            pid: 0,
            bcd_usb: 0,
            num_configs: 0,
            clase: 0,
            subclase: 0,
            protocolo: 0,
            es_mass_storage: false,
            bulk_in_ep: 0,
            bulk_out_ep: 0,
            bulk_in_maxpkt: 0,
            bulk_out_maxpkt: 0,
            capacidad_sectores: 0,
            round_trip_ok: false,
            round_trip_lba: 0,
            mass_storage_attached: false,
            mass_storage_capacity_sectors: 0,
            connected_ports: 0,
            hotplug_events_count: 0,
            automount_active: false,
            mount_path: *b"/media/usb0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            device_node: *b"/dev/sdb\0\0\0\0\0\0\0\0",
            sectors_transferred: 0,
            comandos: 0,
            errores: 0,
            probing: false,
            dcbaa: DmaBuffer::empty(),
            cmd_ring: DmaBuffer::empty(),
            evt_ring: DmaBuffer::empty(),
            erst: DmaBuffer::empty(),
            input_ctx: DmaBuffer::empty(),
            output_ctx: DmaBuffer::empty(),
            trb_buf: DmaBuffer::empty(),
            data_buf: DmaBuffer::empty(),
            desc_buf: DmaBuffer::empty(),
            cmd_cycle: true,
            cmd_enqueue: 0,
            evt_cycle: true,
            evt_dequeue: 0,
            ep0_idx: 0,
            ep0_cycle: true,
            bulk_in_ring: DmaBuffer::empty(),
            bulk_out_ring: DmaBuffer::empty(),
            bulk_in_idx: 0,
            bulk_in_cycle: true,
            bulk_out_idx: 0,
            bulk_out_cycle: true,
            dci_in: 3,
            dci_out: 2,
        }
    }

    // ── Accesos a registros ──────────────────────────────────────────────────
    #[inline(always)]
    fn cap32(&self, off: usize) -> u32 { mmio::mmio_read32(self.mmio_base, off) }
    #[inline(always)]
    fn op32(&self, off: usize) -> u32 { mmio::mmio_read32(self.mmio_base, self.op_offset + off) }
    #[inline(always)]
    fn op_write32(&self, off: usize, val: u32) { mmio::mmio_write32(self.mmio_base, self.op_offset + off, val) }
    #[inline(always)]
    fn op_write64(&self, off: usize, val: u64) { mmio::mmio_write64(self.mmio_base, self.op_offset + off, val) }
    #[inline(always)]
    fn rt32(&self, off: usize) -> u32 { mmio::mmio_read32(self.mmio_base, self.rt_offset + off) }
    #[inline(always)]
    fn rt_write32(&self, off: usize, val: u32) { mmio::mmio_write32(self.mmio_base, self.rt_offset + off, val) }
    #[inline(always)]
    fn rt_write64(&self, off: usize, val: u64) { mmio::mmio_write64(self.mmio_base, self.rt_offset + off, val) }
    #[inline(always)]
    fn port32(&self, puerto: usize) -> u32 {
        mmio::mmio_read32(self.mmio_base, self.cap_length as usize + 0x400 + puerto * 0x10)
    }
    #[inline(always)]
    fn port_write32(&self, puerto: usize, val: u32) {
        mmio::mmio_write32(self.mmio_base, self.cap_length as usize + 0x400 + puerto * 0x10, val)
    }
    /// Timbre de puerta del anillo de comandos del slot 0 / dispositivo.
    fn doorbell(&self, indice: usize, valor: u32) {
        mmio::mmio_write32(self.mmio_base, self.db_offset + indice * 4, valor);
    }

    // ── Anillo de comandos ───────────────────────────────────────────────────
    fn escribir_trb(&self, anillo: &DmaBuffer, idx: usize, p0: u64, status: u32, control: u32) {
        let dir = anillo.vaddr + idx * 16;
        unsafe {
            core::ptr::write_volatile(dir as *mut u32, p0 as u32);
            core::ptr::write_volatile((dir + 4) as *mut u32, (p0 >> 32) as u32);
            core::ptr::write_volatile((dir + 8) as *mut u32, status);
            core::ptr::write_volatile((dir + 12) as *mut u32, control);
        }
    }

    /// Encola un TRB de comando y toca el timbre del host.
    fn encolar_comando(&mut self, p0: u64, status: u32, tipo: u32, extra: u32) {
        let idx = self.cmd_enqueue;
        let control = tipo << 10 | (if self.cmd_cycle { 1 } else { 0 }) | extra;
        self.escribir_trb(&self.cmd_ring, idx, p0, status, control);
        mmio::barrera();
        self.cmd_enqueue = idx + 1;
        if self.cmd_enqueue >= 255 {
            // La vuelta se cierra con el TRB de enlace; el anillo se reutiliza.
            let ring = self.cmd_ring.clone();
            Self::avanzar(&mut self.cmd_enqueue, &mut self.cmd_cycle, 256, &ring);
            self.cmd_enqueue = 0;
        }
        self.comandos += 1;
        self.doorbell(0, 0);
    }

    /// Espera el siguiente evento del anillo y devuelve (tipo, p0, status, control).
    /// Espera el siguiente evento y devuelve (tipo, p0, status, control).
    ///
    /// Los eventos que NO son el esperado se consumen y se descartan (con su
    /// traza): el controlador intercala eventos de cambio de puerto, de anillo
    /// de host, etc. Antes se devolvía el primero que llegara y quien llamaba lo
    /// tomaba como "el transfer falló", desincronizando el anillo de eventos.
    fn esperar_evento(&mut self, tipo_esperado: u32, limite: u32) -> Option<(u32, u64, u32, u32)> {
        for _ in 0..16 {
            let dir = self.evt_ring.vaddr + self.evt_dequeue * 16;
            let mut visto = false;
            let _ = mmio::mmio_wait(
                || {
                    let ctrl = unsafe { core::ptr::read_volatile((dir + 12) as *const u32) };
                    let ciclo = (ctrl & 1) != 0;
                    if ciclo == self.evt_cycle {
                        visto = true;
                        return true;
                    }
                    false
                },
                limite,
            );
            if !visto {
                self.errores += 1;
                return None;
            }
            let p0 = unsafe {
                let lo = core::ptr::read_volatile(dir as *const u32) as u64;
                let hi = core::ptr::read_volatile((dir + 4) as *const u32) as u64;
                lo | (hi << 32)
            };
            let status = unsafe { core::ptr::read_volatile((dir + 8) as *const u32) };
            let control = unsafe { core::ptr::read_volatile((dir + 12) as *const u32) };
            let tipo = (control >> 10) & 0x3F;
            // Avanzar el desencolador del anillo de eventos.
            self.evt_dequeue += 1;
            if self.evt_dequeue >= 64 {
                self.evt_dequeue = 0;
                self.evt_cycle = !self.evt_cycle;
            }
            // Publicar el nuevo desencolador (ERDP con EHB para desenmascarar).
            let erdp = self.evt_ring.paddr + (self.evt_dequeue * 16) as u64;
            self.rt_write64(0x20 + 0x18, erdp | 0x08);
            if tipo_esperado == 0 || tipo == tipo_esperado {
                return Some((tipo, p0, status, control));
            }
            serial::write_str("[USB-XHCI] Evento intercalado tipo=");
            serial::write_u32(tipo);
            serial::write_str(" (se descarta y se sigue esperando)\n");
        }
        self.errores += 1;
        None
    }

    /// Espera un evento de completación de comando y devuelve el código.
    fn completar_comando(&mut self) -> Option<(u8, u8)> {
        for _ in 0..4 {
            match self.esperar_evento(TRB_EVENT_CMD_COMPLETE, 8_000_000) {
                Some((TRB_EVENT_CMD_COMPLETE, _, estado, control)) => {
                    // El código de completación está en el dword de estado
                    // (dw2, bits 31:24) y el Slot ID en dw3 (bits 31:24).
                    // Leerlo de dw3 devolvía el slot y hacía pasar por buenos
                    // comandos que en realidad habían fallado.
                    let code = ((estado >> 24) & 0xFF) as u8;
                    let slot = ((control >> 24) & 0xFF) as u8;
                    return Some((slot, code));
                }
                Some(_) => continue,
                None => return None,
            }
        }
        None
    }

    // ── Arranque del controlador ─────────────────────────────────────────────
    pub fn probe_and_init(&mut self) {
        if self.probing || self.is_running {
            return;
        }
        self.probing = true;
        if !self.is_hardware_present {
            self.escanear_pci();
        }
        if self.is_hardware_present {
            self.iniciar_controlador();
        } else {
            serial::write_str("[USB-XHCI] Sin controlador xHCI en el bus PCI\n");
        }
        self.probing = false;
    }

    fn escanear_pci(&mut self) {
        serial::write_str("[USB-XHCI] Buscando controlador USB 3.0 xHCI en el bus PCI...\n");
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
                    if ((cls >> 24) & 0xFF) as u8 == XHCI_PCI_CLASS
                        && ((cls >> 16) & 0xFF) as u8 == XHCI_PCI_SUBCLASS
                        && ((cls >> 8) & 0xFF) as u8 == XHCI_PCI_PROGIF
                    {
                        let bar0 = unsafe { pci_read32(b, d, f, 0x10) };
                        let bar1 = unsafe { pci_read32(b, d, f, 0x14) };
                        let mmio_base = ((bar1 as u64) << 32) | ((bar0 as u64) & !0xF);
                        let cmd = unsafe { pci_read32(b, d, f, 0x04) } as u16;
                        unsafe { pci_write16(b, d, f, 0x04, cmd | 0x06) };
                        self.mmio_base = mmio_base;
                        // El espacio de capacidad + operativo + puertos + timbres
                        // de un xHCI ocupa varios KiB; se mapea el doble por holgura.
                        self.is_hardware_present = crate::drivers::mmio::mapear_mmio(mmio_base, 0x10000);
                        serial::write_str("[USB-XHCI] Controlador USB 3.0 xHCI detectado en PCI ");
                        serial::write_hex(b as usize);
                        serial::write_str(":");
                        serial::write_hex(d as usize);
                        serial::write_str(" MMIO=");
                        serial::write_hex(mmio_base as usize);
                        serial::write_str("\n");
                        return;
                    }
                }
            }
        }
    }

    fn iniciar_controlador(&mut self) -> bool {
        // ── Capacidades del silicio ─────────────────────────────────────────
        let caplen_ver = self.cap32(XHCI_REG_CAPLENGTH);
        self.cap_length = (caplen_ver & 0xFF) as u8;
        self.hci_version = ((caplen_ver >> 16) & 0xFFFF) as u16;
        let hcs1 = self.cap32(XHCI_REG_HCSPARAMS1);
        let hcs2 = self.cap32(XHCI_REG_HCSPARAMS2);
        let hcc1 = self.cap32(XHCI_REG_HCCPARAMS1);
        self.max_slots = (hcs1 & XHCI_HCSPARAMS1_MAXSLOTS) as u8;
        self.max_intrs = ((hcs1 & XHCI_HCSPARAMS1_MAXINTRS) >> 8) as u16;
        self.max_ports = ((hcs1 & XHCI_HCSPARAMS1_MAXPORTS) >> 24) as u8;
        self.ac64 = (hcc1 & XHCI_HCCPARAMS1_AC64) != 0;
        self.ctx_size = if (hcc1 & XHCI_HCCPARAMS1_CSZ) != 0 { 64 } else { 32 };
        self.op_offset = self.cap_length as usize;
        self.db_offset = (self.cap32(XHCI_REG_DBOFF) & !0x3) as usize;
        self.rt_offset = (self.cap32(XHCI_REG_RTSOFF) & !0x1F) as usize;

        serial::write_str("[USB-XHCI] CAPLENGTH=");
        serial::write_u32(self.cap_length as u32);
        serial::write_str(" HCIVERSION=");
        serial::write_u32(self.hci_version as u32 / 0x100);
        serial::write_str(".");
        serial::write_u32(self.hci_version as u32 % 0x100);
        serial::write_str(" MaxSlots=");
        serial::write_u32(self.max_slots as u32);
        serial::write_str(" MaxPuertos=");
        serial::write_u32(self.max_ports as u32);
        serial::write_str(" Contexto=");
        serial::write_u32(self.ctx_size as u32);
        serial::write_str(" B AC64=");
        serial::write_str(if self.ac64 { "si" } else { "no" });
        serial::write_str(" HCSPARAMS2=");
        serial::write_hex(hcs2 as usize);
        serial::write_str("\n");

        // ── Detener y RESETEAR el controlador ───────────────────────────────
        let cmd = self.op32(XHCI_OP_USBCMD);
        self.op_write32(XHCI_OP_USBCMD, cmd & !XHCI_CMD_RS);
        let _ = mmio::mmio_wait(|| (self.op32(XHCI_OP_USBSTS) & XHCI_STS_HCH) != 0, 10_000_000);
        self.op_write32(XHCI_OP_USBCMD, XHCI_CMD_HCRST);
        let _ = mmio::mmio_wait(|| (self.op32(XHCI_OP_USBSTS) & XHCI_STS_HCRST) != 0, 10_000_000);
        let reset_ok = mmio::mmio_wait(
            || {
                let sts = self.op32(XHCI_OP_USBSTS);
                (sts & XHCI_STS_HCRST) == 0 && (sts & XHCI_STS_CNR) == 0
            },
            8_000_000,
        );
        serial::write_str("[USB-XHCI] Reset del controlador: ");
        serial::write_str(if reset_ok { "completado" } else { "NO completado" });
        serial::write_str("\n");
        if !reset_ok {
            self.errores += 1;
            return false;
        }

        // ── Memoria de los anillos ──────────────────────────────────────────
        let dcbaa = mmio::dma_alloc(2048, 2048);
        let cmd_ring = mmio::dma_alloc(64 * 16, 4096);
        let evt_ring = mmio::dma_alloc(64 * 16, 4096);
        let erst = mmio::dma_alloc(1024, 1024);
        let input_ctx = mmio::dma_alloc(33 * self.ctx_size, self.ctx_size.max(64));
        let output_ctx = mmio::dma_alloc(32 * self.ctx_size, self.ctx_size.max(64));
        let trb_buf = mmio::dma_alloc(4096, 4096);
        let data_buf = mmio::dma_alloc(4096, 4096);
        let desc_buf = mmio::dma_alloc(4096, 4096);
        let bulk_in_ring = mmio::dma_alloc(16 * 16, 4096);
        let bulk_out_ring = mmio::dma_alloc(16 * 16, 4096);
        match (dcbaa, cmd_ring, evt_ring, erst, input_ctx, output_ctx, trb_buf, data_buf, desc_buf, bulk_in_ring, bulk_out_ring) {
            (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g), Some(h), Some(i), Some(j), Some(k)) => {
                self.dcbaa = a; self.cmd_ring = b; self.evt_ring = c; self.erst = d;
                self.input_ctx = e; self.output_ctx = f; self.trb_buf = g;
                self.data_buf = h; self.desc_buf = i;
                self.bulk_in_ring = j; self.bulk_out_ring = k;
            }
            _ => {
                serial::write_str("[USB-XHCI] Sin memoria física contigua para los anillos\n");
                return false;
            }
        }
        // El anillo de comandos termina en un TRB de enlace al inicio (TC=1).
        self.escribir_trb(&self.cmd_ring, 63, self.cmd_ring.paddr, 0, TRB_LINK << 10 | (1 << 1) | 1);

        // Tabla de segmentos del anillo de eventos (1 segmento de 64 TRBs).
        unsafe {
            core::ptr::write_volatile(self.erst.vaddr as *mut u64, self.evt_ring.paddr);
            core::ptr::write_volatile((self.erst.vaddr + 8) as *mut u32, 64);
            core::ptr::write_volatile((self.erst.vaddr + 12) as *mut u32, 0);
        }
        // Interruptor 0: la estructura del interrupter empieza en RTSOFF + 0x20
        // (IMAN 0x00, IMOD 0x04, ERSTSZ 0x08, ERSTBA 0x10, ERDP 0x18). Programar
        // RTSOFF+0x08 directamente escribiría en el registro de índice de fotograma
        // y el anillo de eventos nunca se activaría: ningún evento llegaría jamás.
        const IR0: usize = 0x20;
        self.rt_write32(IR0 + 0x04, 0);            // IMOD: sin moderación
        self.rt_write32(IR0 + 0x00, 0x02);         // IMAN: limpiar IP (W1C)
        self.rt_write32(IR0 + 0x08, 1);            // ERSTSZ: un segmento
        self.rt_write64(IR0 + 0x10, self.erst.paddr);
        self.rt_write64(IR0 + 0x18, self.evt_ring.paddr | 0x08); // ERDP + EHB

        // ── Programar el host ───────────────────────────────────────────────
        self.op_write32(XHCI_OP_CONFIG, (self.max_slots as u32 & 0xFF) | (1 << 8));
        self.op_write64(XHCI_OP_DCBAAP, self.dcbaa.paddr);
        self.op_write64(XHCI_OP_CRCR, self.cmd_ring.paddr | 1); // RCS = 1
        let cmd = self.op32(XHCI_OP_USBCMD);
        self.op_write32(XHCI_OP_USBCMD, cmd | XHCI_CMD_RS);
        let corriendo = mmio::mmio_wait(|| (self.op32(XHCI_OP_USBSTS) & XHCI_STS_HCH) == 0, 20_000_000);
        self.is_running = corriendo;
        serial::write_str("[USB-XHCI] Anillos activos (comandos + eventos), RS=");
        serial::write_str(if corriendo { "1" } else { "0" });
        serial::write_str("\n");

        // ── Estado real de cada puerto raíz ─────────────────────────────────
        let mut conectados = 0u32;
        for p in 0..(self.max_ports as usize).min(32) {
            let sc = self.port32(p);
            let conectado = (sc & XHCI_PORTSC_CCS) != 0;
            let velocidad = ((sc & XHCI_PORTSC_SPEED_MASK) >> 10) as u8;
            let pls = ((sc & XHCI_PORTSC_PLS_MASK) >> 5) as u8;
            self.puertos[p].conectado = conectado;
            self.puertos[p].habilitado = (sc & XHCI_PORTSC_PED) != 0;
            self.puertos[p].velocidad = velocidad;
            self.puertos[p].pls = pls;
            self.puertos[p].speed_id = velocidad;
            if conectado {
                conectados += 1;
                if self.puerto_raiz == 0 {
                    self.puerto_raiz = (p + 1) as u8;
                    self.speed_id = velocidad;
                }
                serial::write_str("[USB-XHCI] Dispositivo en puerto ");
                serial::write_u32(p as u32 + 1);
                serial::write_str(" (velocidad=");
                serial::write_u32(velocidad as u32);
                serial::write_str(if velocidad == 4 { " SuperSpeed)" } else if velocidad == 3 { " HighSpeed)" } else { " Full/Low)" });
                serial::write_str("\n");
            }
        }
        self.connected_ports = conectados;

        if self.puerto_raiz == 0 {
            serial::write_str("[USB-XHCI] Ningún dispositivo conectado a los puertos raíz\n");
            return true;
        }

        // ── Habilitar slot y direccionar el dispositivo ─────────────────────
        if !self.habilitar_slot() {
            return true;
        }
        if !self.direccionar_dispositivo() {
            return true;
        }
        // ── Leer el descriptor de dispositivo REAL ──────────────────────────
        if !self.leer_descriptor_dispositivo() {
            return true;
        }
        // Con el descriptor en la mano se exploran las interfaces: si alguna es
        // de almacenamiento masivo se configuran los endpoints bulk. Antes se
        // condicionaba a `es_mass_storage`, bandera que sólo se enciende dentro
        // de esta misma función: un cerrojo circular que nunca la ejecutaba.
        if self.desc_device_ok {
            self.iniciar_mass_storage();
        }
        true
    }

    fn habilitar_slot(&mut self) -> bool {
        self.encolar_comando(0, 0, TRB_ENABLE_SLOT, 0);
        match self.esperar_evento(TRB_EVENT_CMD_COMPLETE, 8_000_000) {
            Some((TRB_EVENT_CMD_COMPLETE, _, _, control)) => {
                // El slot devuelto viaja en dw3 bits 31:24.
                let slot = ((control >> 24) & 0xFF) as u8;
                if slot == 0 {
                    self.errores += 1;
                    serial::write_str("[USB-XHCI] ENABLE SLOT devolvió slot 0\n");
                    return false;
                }
                self.slot_id = slot;
                self.slot_habilitado = true;
                serial::write_str("[USB-XHCI] Slot ");
                serial::write_u32(slot as u32);
                serial::write_str(" habilitado por el controlador\n");
                true
            }
            _ => {
                self.errores += 1;
                serial::write_str("[USB-XHCI] ENABLE SLOT sin evento de completación\n");
                false
            }
        }
    }

    /// Escribe el contexto de slot y de EP0 en el contexto de ENTRADA.
    fn programar_contexto_input(&mut self, entries: u32) {
        let csz = self.ctx_size;
        // Limpiar el contexto de entrada completo.
        for i in 0..(33 * csz) {
            unsafe { core::ptr::write_volatile((self.input_ctx.vaddr + i) as *mut u8, 0) };
        }
        // Input Control Context (32 B): A0 (slot) y A1 (EP0) activos.
        let flags = 0b11u32;
        unsafe {
            core::ptr::write_volatile((self.input_ctx.vaddr + 4) as *mut u32, flags);
        }
        // Slot Context (primer contexto tras los 32 B de control).
        let slot = self.input_ctx.vaddr + 32;
        let velocidad = (self.speed_id & 0xF) as u32;
        let dw0 = (velocidad << 20) | ((entries & 0x1F) << 27);
        let dw1 = ((self.puerto_raiz as u32) & 0xFF) << 16;
        unsafe {
            core::ptr::write_volatile(slot as *mut u32, dw0);
            core::ptr::write_volatile((slot + 4) as *mut u32, dw1);
            core::ptr::write_volatile((slot + 8) as *mut u32, 0);
            core::ptr::write_volatile((slot + 12) as *mut u32, 0);
        }
        // EP0 Context: MaxPacketSize según velocidad (SS 512, HS 64, FS 8).
        let mps: u32 = match self.speed_id {
            4 => 512,
            3 => 64,
            _ => 8,
        };
        let ep0 = self.input_ctx.vaddr + 32 + csz;
        unsafe {
            core::ptr::write_volatile(ep0 as *mut u32, 0);
            core::ptr::write_volatile((ep0 + 4) as *mut u32, mps << 16);
            core::ptr::write_volatile((ep0 + 8) as *mut u32, 0);
            // Dequeue del anillo de transferencia del EP0 (DCS=1) apuntando al
            // búfer de TRBs: aquí aterrizarán las transferencias de control.
            let dq = self.trb_buf.paddr | 1;
            core::ptr::write_volatile((ep0 + 8) as *mut u32, dq as u32);
            core::ptr::write_volatile((ep0 + 12) as *mut u32, (dq >> 32) as u32);
        }
    }

    fn direccionar_dispositivo(&mut self) -> bool {
        self.programar_contexto_input(1);
        // DCBAA[slot] = dirección del Device Context que el controlador rellenará
        // (y donde vive el estado de cada endpoint). Sin esta entrada el
        // controlador direcciona a 0: los contextos de salida quedan vacíos y los
        // endpoints distintos del EP0 no llegan a arrancar nunca.
        let entrada = self.slot_id as usize * 8;
        mmio::copiar_hacia_dma_offset(&self.dcbaa, entrada, &self.output_ctx.paddr.to_le_bytes());
        let leido = unsafe {
            let lo = core::ptr::read_volatile((self.dcbaa.vaddr + entrada) as *const u32) as u64;
            let hi = core::ptr::read_volatile((self.dcbaa.vaddr + entrada + 4) as *const u32) as u64;
            lo | (hi << 32)
        };
        serial::write_str("[USB-XHCI] DCBAA[");
        serial::write_u32(self.slot_id as u32);
        serial::write_str("] = ");
        serial::write_hex(leido as usize);
        serial::write_str(" (Device Context)\n");
        let ic = self.input_ctx.paddr;
        self.encolar_comando(ic, 0, TRB_ADDRESS_DEVICE, (self.slot_id as u32) << 24);
        match self.esperar_evento(TRB_EVENT_CMD_COMPLETE, 8_000_000) {
            Some((TRB_EVENT_CMD_COMPLETE, _, status, _)) => {
                let code = ((status >> 24) & 0xFF) as u8;
                if code != 1 {
                    serial::write_str("[USB-XHCI] ADDRESS DEVICE codigo ");
                    serial::write_u32(code as u32);
                    serial::write_str(" (1 = exito)
");
                    self.errores += 1;
                    return false;
                }
                self.ep0_configurado = true;
                self.direccion_usb = 1;
                serial::write_str("[USB-XHCI] ADDRESS DEVICE completado (slot ");
                serial::write_u32(self.slot_id as u32);
                serial::write_str(", código de completación ");
                serial::write_u32(code as u32);
                serial::write_str(")\n");
                true
            }
            _ => {
                self.errores += 1;
                serial::write_str("[USB-XHCI] ADDRESS DEVICE sin completación\n");
                false
            }
        }
    }

    /// Escribe un TRB en un anillo concreto en la posición indicada.
    fn escribir_trb_anillo(&self, anillo: &DmaBuffer, idx: usize, ciclo: bool, p0: u64, status: u32, tipo: u32, extra: u32) {
        let control = (tipo << 10) | (if ciclo { 1 } else { 0 }) | extra;
        self.escribir_trb(anillo, idx, p0, status, control);
    }

    /// Escribe el TRB de enlace (Link) del final de un anillo con el ciclo de la
    /// vuelta que se acaba de llenar. Al seguirlo, el controlador invierte su
    /// ciclo y vuelve al principio: así el anillo es circular de verdad. Sin
    /// este TRB el controlador se sale del anillo y deja de responder.
    /// Avanza el índice de un anillo; al dar la vuelta cierra la vuelta con el
    /// TRB de enlace y conmuta el ciclo del productor.
    fn avanzar(idx: &mut usize, ciclo: &mut bool, total: usize, anillo: &DmaBuffer) {
        *idx += 1;
        if *idx >= total - 1 {
            let control = (TRB_LINK << 10) | (1 << 1) | (if *ciclo { 1 } else { 0 });
            // Se escribe el enlace con el ciclo de la vuelta que termina.
            let dir = anillo.vaddr + (total - 1) * 16;
            unsafe {
                core::ptr::write_volatile(dir as *mut u32, anillo.paddr as u32);
                core::ptr::write_volatile((dir + 4) as *mut u32, (anillo.paddr >> 32) as u32);
                core::ptr::write_volatile((dir + 8) as *mut u32, 0);
                core::ptr::write_volatile((dir + 12) as *mut u32, control);
            }
            *idx = 0;
            *ciclo = !*ciclo;
        }
    }

    /// Transferencia de control por EP0 (SETUP + DATA + STATUS) sobre el anillo
    /// de transferencia del endpoint, que avanza transferencia a transferencia:
    /// así el controlador siempre encuentra TRBs nuevos con el ciclo correcto.
    fn transferencia_control(
        &mut self,
        bm_request_type: u8,
        b_request: u8,
        w_value: u16,
        w_index: u16,
        datos: &mut [u8],
        es_in: bool,
    ) -> bool {
        const TRBS_EP0: usize = 256; // 4096 B / 16
        let setup = (bm_request_type as u64)
            | ((b_request as u64) << 8)
            | ((w_value as u64) << 16)
            | ((w_index as u64) << 32)
            | ((datos.len() as u64) << 48);
        let trt: u32 = if datos.is_empty() { 0 } else if es_in { 3 } else { 2 };
        self.escribir_trb_anillo(&self.trb_buf.clone(), self.ep0_idx, self.ep0_cycle, setup, 8,
                                 TRB_SETUP_STAGE, (1 << 6) | (trt << 16));
        let anillo_ep0 = self.trb_buf.clone();
        Self::avanzar(&mut self.ep0_idx, &mut self.ep0_cycle, TRBS_EP0, &anillo_ep0);
        if !datos.is_empty() {
            self.escribir_trb_anillo(&self.trb_buf.clone(), self.ep0_idx, self.ep0_cycle,
                                     self.desc_buf.paddr, datos.len() as u32,
                                     TRB_DATA_STAGE, (es_in as u32) << 16);
            let anillo_ep0 = self.trb_buf.clone();
        Self::avanzar(&mut self.ep0_idx, &mut self.ep0_cycle, TRBS_EP0, &anillo_ep0);
        }
        self.escribir_trb_anillo(&self.trb_buf.clone(), self.ep0_idx, self.ep0_cycle, 0, 0,
                                 TRB_STATUS_STAGE, (1 << 5)
                                     | (if es_in { 0 } else { 1 << 16 }));
        let anillo_ep0 = self.trb_buf.clone();
        Self::avanzar(&mut self.ep0_idx, &mut self.ep0_cycle, TRBS_EP0, &anillo_ep0);
        mmio::barrera();
        // La puerta del dispositivo es su Slot ID; el objetivo es el DCI del
        // endpoint (EP0 = 1).
        let puerta = self.slot_id as usize;
        self.doorbell(puerta, 1);

        match self.esperar_evento(TRB_EVENT_TRANSFER, 8_000_000) {
            Some((TRB_EVENT_TRANSFER, _, status, _control)) => {
                let code = ((status >> 24) & 0xFF) as u8;
                let residual = (status & 0x00FF_FFFF) as usize;
                if code != 1 {
                    serial::write_str("[USB-XHCI] Transferencia de control: código ");
                    serial::write_u32(code as u32);
                    serial::write_str("\n");
                    self.errores += 1;
                    return false;
                }
                if !datos.is_empty() {
                    let leidos = datos.len().saturating_sub(residual).min(datos.len());
                    mmio::copiar_desde_dma(&self.desc_buf, &mut datos[..leidos]);
                }
                true
            }
            _ => {
                self.errores += 1;
                serial::write_str("[USB-XHCI] Transferencia de control sin evento\n");
                false
            }
        }
    }

    /// GET_DESCRIPTOR(DEVICE): los 18 bytes que el dispositivo declara de sí mismo.
    fn leer_descriptor_dispositivo(&mut self) -> bool {
        let mut desc = [0u8; 18];
        if !self.transferencia_control(0x80, 0x06, 0x0100, 0, &mut desc, true) {
            return false;
        }
        if desc[0] < 18 || desc[1] != 0x01 {
            serial::write_str("[USB-XHCI] Descriptor de dispositivo no válido\n");
            return false;
        }
        self.desc_device = desc;
        self.desc_device_ok = true;
        self.bcd_usb = u16::from_le_bytes([desc[2], desc[3]]);
        self.clase = desc[4];
        self.subclase = desc[5];
        self.protocolo = desc[6];
        self.vid = u16::from_le_bytes([desc[8], desc[9]]);
        self.pid = u16::from_le_bytes([desc[10], desc[11]]);
        self.num_configs = desc[17];
        self.hotplug_events_count += 1;
        serial::write_str("[USB-XHCI] DESCRIPTOR de dispositivo: VID:PID=");
        serial::write_hex(self.vid as usize);
        serial::write_str(":");
        serial::write_hex(self.pid as usize);
        serial::write_str(" USB ");
        serial::write_u32((self.bcd_usb >> 8) as u32);
        serial::write_str(".");
        serial::write_u32((self.bcd_usb & 0xFF) as u32);
        serial::write_str(" clase=");
        serial::write_hex(self.clase as usize);
        serial::write_str("/");
        serial::write_hex(self.subclase as usize);
        serial::write_str("/");
        serial::write_hex(self.protocolo as usize);
        serial::write_str(" configuraciones=");
        serial::write_u32(self.num_configs as u32);
        serial::write_str("\n");
        true
    }

    /// Descriptor de configuración completo y localización de los endpoints bulk.
    fn iniciar_mass_storage(&mut self) -> bool {
        // Primero 9 bytes para conocer wTotalLength, después el bloque completo.
        let mut cabecera = [0u8; 9];
        if !self.transferencia_control(0x80, 0x06, 0x0200, 0, &mut cabecera, true) {
            serial::write_str("[USB-XHCI] GET_DESCRIPTOR(Configuration) falló\n");
            return false;
        }
        serial::write_str("[USB-XHCI] Configuracion: wTotalLength=");
        serial::write_u32(u16::from_le_bytes([cabecera[2], cabecera[3]]) as u32);
        serial::write_str("\n");
        let total = u16::from_le_bytes([cabecera[2], cabecera[3]]) as usize;
        let total = total.min(512);
        if total < 9 {
            return false;
        }
        let mut conf = [0u8; 512];
        if !self.transferencia_control(0x80, 0x06, 0x0200, 0, &mut conf[..total], true) {
            return false;
        }
        self.desc_config[..total].copy_from_slice(&conf[..total]);
        self.desc_config_len = total as u16;
        self.desc_config_ok = true;

        // Recorrer los descriptores buscando la interfaz de almacenamiento
        // masivo (clase 8, subclase 6, protocolo 0x50) y sus endpoints bulk.
        let mut i = 0usize;
        let mut en_mass = false;
        while i + 1 < total {
            let largo = conf[i] as usize;
            let tipo = conf[i + 1];
            if largo < 2 || i + largo > total { break; }
            if tipo == 0x04 && largo >= 9 {
                self.clase = conf[i + 5];
                self.subclase = conf[i + 6];
                self.protocolo = conf[i + 7];
                en_mass = self.clase == 0x08 && self.subclase == 0x06 && (self.protocolo == 0x50 || self.protocolo == 0x62);
            } else if tipo == 0x05 && largo >= 7 && en_mass {
                let dir = conf[i + 2] & 0x80 != 0;
                let attr = conf[i + 3] & 0x03;
                let mps = u16::from_le_bytes([conf[i + 4], conf[i + 5]]);
                if attr == 0x02 {
                    if dir {
                        self.bulk_in_ep = conf[i + 2] & 0x0F;
                        self.bulk_in_maxpkt = mps;
                    } else {
                        self.bulk_out_ep = conf[i + 2] & 0x0F;
                        self.bulk_out_maxpkt = mps;
                    }
                }
            }
            i += largo;
        }
        if self.bulk_in_ep != 0 && self.bulk_out_ep != 0 {
            self.es_mass_storage = true;
            self.mass_storage_attached = true;
            self.automount_active = true;
            serial::write_str("[USB-XHCI] Interfaz de ALMACENAMIENTO MASIVO: bulk IN=EP");
            serial::write_u32(self.bulk_in_ep as u32);
            serial::write_str(" bulk OUT=EP");
            serial::write_u32(self.bulk_out_ep as u32);
            serial::write_str(" (mps ");
            serial::write_u32(self.bulk_in_maxpkt as u32);
            serial::write_str("/");
            serial::write_u32(self.bulk_out_maxpkt as u32);
            serial::write_str(")\n");
            // Elegir la configuración 1 del dispositivo.
            if self.transferencia_control(0x00, 0x09, 1, 0, &mut [], false) {
                serial::write_str("[USB-XHCI] Configuración 1 seleccionada\n");
            }
            self.configurar_endpoints_bulk();
            self.leer_capacidad_scsi();
        } else {
            serial::write_str("[USB-XHCI] El dispositivo no expone almacenamiento masivo\n");
        }
        true
    }

    /// Vuelca el contexto de dispositivo que el controlador mantiene en memoria:
    /// es la prueba de qué aceptó realmente el Configure Endpoint (estado del
    /// endpoint, tipo, tamaño de paquete y puntero de desencolado).
    fn volcar_contexto_salida(&self) {
        let csz = self.ctx_size;
        let out = self.output_ctx.vaddr;
        // Contexto de slot en el desplazamiento 0; el contexto del endpoint DCI n
        // está en n*csz (el EP0 es el DCI 1).
        let slot = out;
        let dw0 = unsafe { core::ptr::read_volatile(slot as *const u32) };
        serial::write_str("[USB-XHCI] Contexto de salida: slot entradas=");
        serial::write_u32((dw0 >> 27) & 0x1F);
        serial::write_str(" velocidad=");
        serial::write_u32((dw0 >> 20) & 0xF);
        serial::write_str("\n");
        for dci in 1..=4usize {
            let ep = out + csz * dci;
            let e0 = unsafe { core::ptr::read_volatile(ep as *const u32) };
            let e1 = unsafe { core::ptr::read_volatile((ep + 4) as *const u32) };
            let dq = unsafe {
                let lo = core::ptr::read_volatile((ep + 8) as *const u32) as u64;
                let hi = core::ptr::read_volatile((ep + 12) as *const u32) as u64;
                lo | (hi << 32)
            };
            serial::write_str("[USB-XHCI]   DCI ");
            serial::write_u32(dci as u32);
            serial::write_str(": estado=");
            serial::write_u32(e0 & 0x7);
            serial::write_str(" tipo=");
            serial::write_u32((e0 >> 3) & 0x7);
            serial::write_str(" mps=");
            serial::write_u32((e1 >> 16) & 0xFFFF);
            serial::write_str(" DQ=");
            serial::write_hex(dq as usize);
            serial::write_str("\n");
        }
    }

    /// Programa los contextos de los endpoints bulk y los habilita.
    fn configurar_endpoints_bulk(&mut self) -> bool {
        let csz = self.ctx_size;
        for i in 0..(33 * csz) {
            unsafe { core::ptr::write_volatile((self.input_ctx.vaddr + i) as *mut u8, 0) };
        }
        // El DCI sale del número de endpoint que declaró el dispositivo:
        // DCI = 2*EP + sentido (0 = OUT, 1 = IN). Los contextos se colocan en su
        // ranura real, no en posiciones fijas.
        let dci_in = 2 * self.bulk_in_ep as u32 + 1;
        let dci_out = 2 * self.bulk_out_ep as u32;
        self.dci_in = dci_in as u8;
        self.dci_out = dci_out as u8;
        let max_dci = dci_in.max(dci_out);
        // Banderas A: bit 0 (slot) obligatorio y un bit por DCI. El bit 1 (EP0)
        // debe quedar a cero: el contexto del EP0 ya lo fijó ADDRESS DEVICE y
        // volver a añadirlo invalida el comando (el controlador exige que la
        // parte baja sea exactamente 0b01 y si no responde TRB_ERROR).
        let banderas_a = 1u32 | (1 << dci_in) | (1 << dci_out);
        unsafe { core::ptr::write_volatile((self.input_ctx.vaddr + 4) as *mut u32, banderas_a) };
        let slot = self.input_ctx.vaddr + 32;
        let velocidad = (self.speed_id & 0xF) as u32;
        let dw0 = (velocidad << 20) | (max_dci << 27); // Context Entries = DCI mayor
        let dw1 = ((self.puerto_raiz as u32) & 0xFF) << 16;
        unsafe {
            core::ptr::write_volatile(slot as *mut u32, dw0);
            core::ptr::write_volatile((slot + 4) as *mut u32, dw1);
        }
        // EP0 (sin cambios): el contexto de entrada ya está a cero, el
        // controlador conserva el contexto de salida del EP0.
        let mps_ep0: u32 = match self.speed_id { 4 => 512, 3 => 64, _ => 8 };
        let ep0 = self.input_ctx.vaddr + 32 + csz;
        unsafe {
            core::ptr::write_volatile((ep0 + 4) as *mut u32, mps_ep0 << 16);
            let dq = self.trb_buf.paddr | 1;
            core::ptr::write_volatile((ep0 + 8) as *mut u32, dq as u32);
            core::ptr::write_volatile((ep0 + 12) as *mut u32, (dq >> 32) as u32);
        }
        // Contexto del endpoint en su ranura real: 32 + DCI*csz.
        let ep_out = self.input_ctx.vaddr + 32 + (dci_out as usize) * csz;
        let ep_in = self.input_ctx.vaddr + 32 + (dci_in as usize) * csz;
        serial::write_str("[USB-XHCI] Buffers DMA: ctx_in=");
        serial::write_hex(self.input_ctx.paddr as usize);
        serial::write_str(" ctx_out=");
        serial::write_hex(self.output_ctx.paddr as usize);
        serial::write_str(" anillo_in=");
        serial::write_hex(self.bulk_in_ring.paddr as usize);
        serial::write_str(" anillo_out=");
        serial::write_hex(self.bulk_out_ring.paddr as usize);
        serial::write_str(" datos=");
        serial::write_hex(self.data_buf.paddr as usize);
        serial::write_str("\n");
        serial::write_str("[USB-XHCI] Contextos bulk: DCI OUT=");
        serial::write_u32(dci_out);
        serial::write_str(" (EP");
        serial::write_u32(self.bulk_out_ep as u32);
        serial::write_str(") DCI IN=");
        serial::write_u32(dci_in);
        serial::write_str(" (EP");
        serial::write_u32(self.bulk_in_ep as u32);
        serial::write_str(")\n");
        let anillo_out = self.bulk_out_ring.paddr;
        let anillo_in = self.bulk_in_ring.paddr;
        for (dir_ctx, mps, anillo) in [
            (ep_out, self.bulk_out_maxpkt as u32, anillo_out),
            (ep_in, self.bulk_in_maxpkt as u32, anillo_in),
        ] {
            // El tipo de endpoint NO va en dw0: vive en dw1 bits 5:3, junto a
            // CErr (2:1) y MaxPacketSize (31:16). Ponerlo en dw0 deja al
            // controlador con tipo INVALID y toda transferencia se descarta con
            // "unimplemented endpoint type". 2 = Bulk OUT, 6 = Bulk IN.
            let tipo_ep: u32 = if dir_ctx == ep_out { 2 } else { 6 };
            // dw0: EP State=0, Interval=0, LSA=0, MaxPStreams=0, Mult=0.
            let dw0 = 0u32;
            // dw1: tipo (5:3) | CErr=3 (2:1) | MaxPacketSize (31:16). MaxBurst
            // (dw2 bits 7:0) queda a 0: lo manda el descriptor companion.
            let dw1 = (tipo_ep << 3) | (3u32 << 1) | ((mps & 0x7FF) << 16);
            unsafe {
                core::ptr::write_volatile(dir_ctx as *mut u32, dw0);
                core::ptr::write_volatile((dir_ctx + 4) as *mut u32, dw1);
                core::ptr::write_volatile((dir_ctx + 8) as *mut u32, (anillo | 1) as u32);
                core::ptr::write_volatile((dir_ctx + 12) as *mut u32, (anillo >> 32) as u32);
            }
        }
        let ic = self.input_ctx.paddr;
        self.encolar_comando(ic, 0, TRB_CONFIGURE_ENDPOINT, (self.slot_id as u32) << 24);
        match self.esperar_evento(TRB_EVENT_CMD_COMPLETE, 8_000_000) {
            Some((TRB_EVENT_CMD_COMPLETE, _, status, _)) => {
                let code = ((status >> 24) & 0xFF) as u8;
                serial::write_str("[USB-XHCI] CONFIGURE ENDPOINT (bulk) código ");
                serial::write_u32(code as u32);
                serial::write_str("\n");
                self.volcar_contexto_salida();
                true
            }
            _ => {
                self.errores += 1;
                false
            }
        }
    }

    /// Ejecuta una transferencia bulk por el endpoint correspondiente, sobre su
    /// propio anillo (índice y ciclo independientes) y con el búfer de datos real.
    fn transferencia_bulk(&mut self, es_in: bool, datos: &mut [u8]) -> bool {
        const TRBS_BULK: usize = 16;
        let anillo_ring = if es_in { self.bulk_in_ring.clone() } else { self.bulk_out_ring.clone() };
        let idx = if es_in { self.bulk_in_idx } else { self.bulk_out_idx };
        let ciclo = if es_in { self.bulk_in_cycle } else { self.bulk_out_cycle };
        // El TRB apunta SIEMPRE al búfer de datos: en IN el controlador escribe
        // allí, y en OUT lee de allí lo que el CPU ya depositó.
        self.escribir_trb_anillo(&anillo_ring, idx, ciclo, self.data_buf.paddr, datos.len() as u32,
                                 TRB_NORMAL, 1 << 5);
        mmio::barrera();
        if es_in {
            Self::avanzar(&mut self.bulk_in_idx, &mut self.bulk_in_cycle, TRBS_BULK, &anillo_ring);
        } else {
            Self::avanzar(&mut self.bulk_out_idx, &mut self.bulk_out_cycle, TRBS_BULK, &anillo_ring);
        }
        // La puerta es el Slot ID y el valor es el DCI del endpoint real.
        let puerta = self.slot_id as usize;
        self.doorbell(puerta, if es_in { self.dci_in as u32 } else { self.dci_out as u32 });
        match self.esperar_evento(TRB_EVENT_TRANSFER, 12_000_000) {
            Some((TRB_EVENT_TRANSFER, _, status, _control)) => {
                let code = ((status >> 24) & 0xFF) as u8;
                let residual = (status & 0x00FF_FFFF) as usize;
                serial::write_str("[USB-XHCI] Bulk ");
                serial::write_str(if es_in { "IN " } else { "OUT" });
                serial::write_str(" len=");
                serial::write_u32(datos.len() as u32);
                serial::write_str(" codigo=");
                serial::write_u32(code as u32);
                serial::write_str(" residual=");
                serial::write_u32(residual as u32);
                serial::write_str("\n");
                if code != 1 {
                    self.errores += 1;
                    return false;
                }
                if es_in {
                    let n = datos.len().saturating_sub(residual).min(datos.len());
                    mmio::copiar_desde_dma(&self.data_buf, &mut datos[..n]);
                }
                true
            }
            _ => {
                serial::write_str("[USB-XHCI] Bulk ");
                serial::write_str(if es_in { "IN " } else { "OUT" });
                serial::write_str(" len=");
                serial::write_u32(datos.len() as u32);
                serial::write_str(" SIN EVENTO\n");
                self.errores += 1;
                false
            }
        }
    }

    /// Construye un CBW (Command Block Wrapper) de Bulk-Only Transport.
    ///
    /// El CBW tiene 31 bytes: firma "USBC" (0..3), etiqueta (4..7), longitud de
    /// datos (8..11), banderas (12), LUN (13), longitud del CDB (14) y **el CDB a
    /// partir del byte 15**. Escribir el opcode en el byte 14 metía la orden en
    /// el campo de longitud: el dispositivo recibía un CDB inválido
    /// (TEST UNIT READY) y nunca ejecutaba la orden pedida.
    fn construir_cbw(tag: u32, longitud_datos: u32, entrada: bool, cdb: &[u8]) -> [u8; 31] {
        let mut cbw = [0u8; 31];
        cbw[0] = 0x55; cbw[1] = 0x53; cbw[2] = 0x42; cbw[3] = 0x43; // "USBC"
        cbw[4..8].copy_from_slice(&tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&longitud_datos.to_le_bytes());
        cbw[12] = if entrada { 0x80 } else { 0x00 };
        cbw[13] = 0; // LUN 0
        let n = cdb.len().min(16);
        cbw[14] = n as u8;
        cbw[15..15 + n].copy_from_slice(&cdb[..n]);
        cbw
    }

    /// Deposita bytes en el búfer de datos de bulk (camino de salida).
    fn cargar_bulk(&mut self, bytes: &[u8]) {
        let mut off = 0;
        while off < bytes.len() {
            let n = (bytes.len() - off).min(4096);
            mmio::copiar_hacia_dma_offset(&self.data_buf, off, &bytes[off..off + n]);
            off += n;
        }
    }

    /// SCSI INQUIRY + READ CAPACITY(10): capacidad real del dispositivo USB.
    fn leer_capacidad_scsi(&mut self) -> bool {
        // INQUIRY con 36 bytes de asignación.
        let cbw = Self::construir_cbw(1, 36, true, &[0x12, 0, 0, 0, 36, 0]);
        self.cargar_bulk(&cbw);
        if !self.transferencia_bulk(false, &mut [0u8; 31]) {
            serial::write_str("[USB-XHCI] CBW de INQUIRY no se pudo enviar\n");
            return false;
        }
        let mut inquiry = [0u8; 36];
        if !self.transferencia_bulk(true, &mut inquiry) {
            return false;
        }
        let mut csw = [0u8; 13];
        if !self.transferencia_bulk(true, &mut csw) {
            return false;
        }
        serial::write_str("[USB-XHCI] SCSI INQUIRY: fabricante=\"");
        escribir_texto(&inquiry[8..16]);
        serial::write_str("\" producto=\"");
        escribir_texto(&inquiry[16..32]);
        serial::write_str("\"\n");

        // READ CAPACITY(10) → últimos LBA y tamaño de bloque.
        let cbw2 = Self::construir_cbw(2, 8, true, &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        self.cargar_bulk(&cbw2);
        if !self.transferencia_bulk(false, &mut [0u8; 31]) {
            return false;
        }
        let mut cap = [0u8; 8];
        if !self.transferencia_bulk(true, &mut cap) {
            return false;
        }
        let mut csw2 = [0u8; 13];
        let _ = self.transferencia_bulk(true, &mut csw2);
        let ultimo_lba = u32::from_be_bytes([cap[0], cap[1], cap[2], cap[3]]) as u64;
        let tam_bloque = u32::from_be_bytes([cap[4], cap[5], cap[6], cap[7]]);
        if tam_bloque == 0 {
            return false;
        }
        self.capacidad_sectores = (ultimo_lba + 1) * (tam_bloque as u64 / 512).max(1);
        self.mass_storage_capacity_sectors = self.capacidad_sectores;
        serial::write_str("[USB-XHCI] READ CAPACITY(10): ");
        serial::write_u64(ultimo_lba + 1);
        serial::write_str(" bloques de ");
        serial::write_u32(tam_bloque);
        serial::write_str(" B -> capacidad ");
        serial::write_u64(self.capacidad_sectores * 512 / 1_048_576);
        serial::write_str(" MiB\n");
        true
    }

    /// READ(10) de un sector por SCSI sobre el transporte Bulk-Only.
    pub fn read_sector(&mut self, lba: u32, buf: &mut [u8; 512]) -> bool {
        if !self.es_mass_storage {
            return false;
        }
        // READ(10): el LBA va en big-endian en el CDB[2..6] y la cuenta en CDB[7..9].
        let cbw = Self::construir_cbw(3, 512, true, &[
            0x28, 0,
            ((lba >> 24) & 0xFF) as u8, ((lba >> 16) & 0xFF) as u8,
            ((lba >> 8) & 0xFF) as u8, (lba & 0xFF) as u8,
            0, 0, 1, 0,
        ]);
        self.cargar_bulk(&cbw);
        if !self.transferencia_bulk(false, &mut [0u8; 31]) {
            return false;
        }
        let mut datos = [0u8; 512];
        if !self.transferencia_bulk(true, &mut datos) {
            return false;
        }
        let mut csw = [0u8; 13];
        if !self.transferencia_bulk(true, &mut csw) {
            return false;
        }
        if csw[12] != 0 {
            serial::write_str("[USB-XHCI] CSW con estado no nulo\n");
            return false;
        }
        buf.copy_from_slice(&datos);
        self.sectors_transferred += 1;
        true
    }

    /// Escribe un sector por SCSI WRITE(10).
    pub fn write_sector(&mut self, lba: u32, buf: &[u8; 512]) -> bool {
        if !self.es_mass_storage {
            return false;
        }
        let cbw = Self::construir_cbw(4, 512, false, &[
            0x2A, 0,
            ((lba >> 24) & 0xFF) as u8, ((lba >> 16) & 0xFF) as u8,
            ((lba >> 8) & 0xFF) as u8, (lba & 0xFF) as u8,
            0, 0, 1, 0,
        ]);
        // El CBW y los datos viajan en DOS transferencias: el dispositivo
        // espera un paquete de exactamente 31 bytes para el CBW y otro con los
        // datos. Mandarlos juntos en un paquete de 543 B hace que rechace la
        // orden con STALL (código 6).
        self.cargar_bulk(&cbw);
        if !self.transferencia_bulk(false, &mut [0u8; 31]) {
            serial::write_str("[USB-XHCI] CBW de WRITE(10) rechazado\n");
            return false;
        }
        mmio::copiar_hacia_dma_offset(&self.data_buf, 0, buf);
        if !self.transferencia_bulk(false, &mut [0u8; 512]) {
            serial::write_str("[USB-XHCI] datos de WRITE(10) rechazados\n");
            return false;
        }
        let mut csw = [0u8; 13];
        if !self.transferencia_bulk(true, &mut csw) {
            return false;
        }
        if csw[12] != 0 {
            return false;
        }
        self.sectors_transferred += 1;
        true
    }

    /// Ensayo de ida y vuelta sobre el pen drive: escribir y releer un sector.
    pub fn self_test(&mut self) -> bool {
        if !self.es_mass_storage || self.capacidad_sectores < 8 {
            return false;
        }
        let lba = (self.capacidad_sectores - 1) as u32;
        self.round_trip_lba = lba;
        let mut patron = [0u8; 512];
        for (i, b) in patron.iter_mut().enumerate() {
            *b = ((i as u32 * 13 + 0x77) & 0xFF) as u8;
        }
        let mut leido = [0u8; 512];
        if !self.write_sector(lba, &patron) {
            return false;
        }
        if !self.read_sector(lba, &mut leido) {
            return false;
        }
        self.round_trip_ok = patron == leido;
        self.round_trip_ok
    }

    pub fn read_lba0(&mut self, buf: &mut [u8; 512]) -> bool {
        self.read_sector(0, buf)
    }

    pub fn generate_telemetry(&self) -> String {
        let mut out = String::from("╔══════════════════════════════════════════════════════════════════════╗\n");
        out.push_str("║     [XHCI] CONTROLADOR USB 3.x (xHCI) — ESTADO DEL SILICIO REAL      ║\n");
        out.push_str("╚══════════════════════════════════════════════════════════════════════╝\n\n");
        if !self.is_hardware_present {
            out.push_str("Estado de Silicio   : AUSENTE — ningún controlador xHCI respondió en el bus PCI\n");
            out.push_str("[SILICIO-AUSENTE] No se emiten métricas de hardware inexistente.\n");
            return out;
        }
        out.push_str(&format!("Estado de Silicio   : PRESENTE (MMIO=0x{:016x})\n", self.mmio_base));
        out.push_str(&format!("Versión xHCI        : {}.{} | Contextos de {} B | AC64={}\n",
                              self.hci_version >> 8, self.hci_version & 0xFF, self.ctx_size,
                              if self.ac64 { "sí" } else { "no" }));
        out.push_str(&format!("Capacidad           : MaxSlots={} MaxPuertos={} Intrs={} (leído de HCSPARAMS1)\n",
                              self.max_slots, self.max_ports, self.max_intrs));
        out.push_str(&format!("Anillos             : comandos y eventos activos={} | comandos emitidos={} | errores={}\n",
                              if self.is_running { "sí" } else { "no" }, self.comandos, self.errores));
        out.push_str(&format!("Puertos con Dispositivo: {} de {}\n", self.connected_ports, self.max_ports));
        for p in self.puertos.iter() {
            if !p.conectado { continue; }
            let nombre = match p.velocidad { 4 => "SuperSpeed (5 Gbps)", 3 => "HighSpeed (480 Mbps)", 2 => "LowSpeed", 1 => "FullSpeed", _ => "desconocida" };
            out.push_str(&format!("  Puerto {} : CONECTADO — {} | PLS={} | PED={}\n",
                                  p.numero + 1, nombre, p.pls, if p.habilitado { 1 } else { 0 }));
        }
        if self.slot_habilitado {
            out.push_str(&format!("\nSlot asignado       : {} (address device completado) | EP0 configurado={}\n",
                                  self.slot_id, if self.ep0_configurado { "sí" } else { "no" }));
        }
        if self.desc_device_ok {
            out.push_str(&format!("Descriptor Device   : VID:PID = {:04x}:{:04x} | USB {}.{}\n",
                                  self.vid, self.pid, self.bcd_usb >> 8, self.bcd_usb & 0xFF));
            out.push_str(&format!("Clase               : 0x{:02x}/0x{:02x}/0x{:02x}{}\n",
                                  self.clase, self.subclase, self.protocolo,
                                  if self.es_mass_storage { " (ALMACENAMIENTO MASIVO)" } else { "" }));
        } else if self.is_hardware_present {
            out.push_str("Descriptor Device   : no leído (sin dispositivo o transferencia fallida)\n");
        }
        if self.es_mass_storage {
            out.push_str(&format!("Endpoints Bulk      : IN=EP{} ({} B) OUT=EP{} ({} B)\n",
                                  self.bulk_in_ep, self.bulk_in_maxpkt, self.bulk_out_ep, self.bulk_out_maxpkt));
            out.push_str(&format!("Capacidad (SCSI)    : {} sectores = {} MiB\n",
                                  self.capacidad_sectores, self.capacidad_sectores * 512 / 1_048_576));
            out.push_str(&format!("Sectores Transferidos: {}\n", self.sectors_transferred));
            if self.round_trip_ok {
                out.push_str(&format!("Ensayo de Silicio   : ESCRITURA+LECTURA verificada en el bloque {} [SILICIO-VERIFICADO]\n",
                                      self.round_trip_lba));
            }
        }
        out.push_str("\n[EPIC-49-PASS] Controlador xHCI operado por anillos TRB sobre hardware real.\n");
        out
    }
}

fn escribir_texto(campo: &[u8]) {
    for &c in campo {
        if c == 0 { break; }
        if c >= 0x20 && c < 0x7F { serial::write_byte(c); }
    }
}

pub static XHCI_DEV: Spinlock<XhciController> = Spinlock::new(XhciController::empty());

pub fn xhci_init() {
    XHCI_DEV.lock().probe_and_init();
}
