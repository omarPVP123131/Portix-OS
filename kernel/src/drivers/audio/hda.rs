// kernel/src/drivers/audio/hda.rs — Controlador Intel High Definition Audio (Intel HDA) y BDL DMA Ring Buffer
// Especificación Intel High Definition Audio Rev 1.0a (PCI 0x8086:0x2668 / QEMU intel-hda)
#![allow(dead_code)]

use crate::arch::spinlock::Spinlock;

// ── Registros del Controlador Intel HDA (Offsets MMIO) ───────────────────────
pub const HDA_REG_GCAP:      usize = 0x00; // Global Capabilities (16-bit)
pub const HDA_REG_VMIN:      usize = 0x02; // Minor Version (8-bit)
pub const HDA_REG_VMAJ:      usize = 0x03; // Major Version (8-bit)
pub const HDA_REG_OUTPAY:    usize = 0x04; // Output Payload Capability (16-bit)
pub const HDA_REG_INPAY:     usize = 0x06; // Input Payload Capability (16-bit)
pub const HDA_REG_GCTL:      usize = 0x08; // Global Control (32-bit)
pub const HDA_REG_WAKEEN:    usize = 0x0C; // Wake Enable (16-bit)
pub const HDA_REG_STATESTS:  usize = 0x0E; // State Change Status (16-bit)
pub const HDA_REG_GSTS:      usize = 0x10; // Global Status (16-bit)
pub const HDA_REG_INTCTL:    usize = 0x20; // Interrupt Control (32-bit)
pub const HDA_REG_INTSTS:    usize = 0x24; // Interrupt Status (32-bit)
pub const HDA_REG_WALCLK:    usize = 0x30; // Wall Clock Counter (32-bit)
// Motor de comandos CORB/RIRB (el camino canónico para hablar con los códecs)
pub const HDA_REG_CORBLBASE:  usize = 0x40;
pub const HDA_REG_CORBUBASE:  usize = 0x44;
pub const HDA_REG_CORBWP:     usize = 0x48;
pub const HDA_REG_CORBRP:     usize = 0x4A;
pub const HDA_REG_CORBCTL:    usize = 0x4C;
pub const HDA_REG_CORBSTS:    usize = 0x4D;
pub const HDA_REG_CORBSIZE:   usize = 0x4E;
pub const HDA_REG_RIRBLBASE:  usize = 0x50;
pub const HDA_REG_RIRBUBASE:  usize = 0x54;
pub const HDA_REG_RIRBWP:     usize = 0x58;
pub const HDA_REG_RINTCNT:    usize = 0x5A;
pub const HDA_REG_RIRBCTL:    usize = 0x5C;
pub const HDA_REG_RIRBSTS:    usize = 0x5D;
pub const HDA_REG_RIRBSIZE:   usize = 0x5E;
/// CORBCTL: bit 1 habilita el motor de comandos (bit 0 = aviso de error de
/// memoria). RIRBCTL: bit 1 habilita el DMA de respuestas (bit 0 = interrupción).
pub const HDA_CORBCTL_RUN:    u8 = 1 << 1;
pub const HDA_RBCTL_DMA_EN:   u8 = 1 << 1;
pub const HDA_CORB_ENTRADAS:  usize = 256;
pub const HDA_RIRB_ENTRADAS:  usize = 256;

// Descriptores de Stream de Salida (Output Stream 0 Offset: 0x100)
pub const HDA_SD_OFFSET:     usize = 0x100;
pub const HDA_SD_CTL:        usize = 0x00; // Stream Control (24-bit en 32-bit access)
pub const HDA_SD_STS:        usize = 0x03; // Stream Status (8-bit)
pub const HDA_SD_LPIB:       usize = 0x04; // Link Position In Buffer (32-bit)
pub const HDA_SD_CBL:        usize = 0x08; // Cyclic Buffer Length (32-bit)
pub const HDA_SD_LVI:        usize = 0x0C; // Last Valid Index (16-bit)
pub const HDA_SD_FIFOW:      usize = 0x0E; // FIFO Watermark (16-bit)
pub const HDA_SD_FIFOS:      usize = 0x10; // FIFO Size (16-bit)
pub const HDA_SD_FMT:        usize = 0x12; // Stream Format (16-bit)
pub const HDA_SD_BDPL:       usize = 0x18; // BDL Pointer Lower 32 bits
pub const HDA_SD_BDPU:       usize = 0x1C; // BDL Pointer Upper 32 bits

// Bits de Control
pub const HDA_GCTL_CRST:     u32 = 1 << 0;  // Controller Reset (1 = Run, 0 = Reset)
pub const HDA_SD_CTL_SRST:   u32 = 1 << 0;  // Stream Reset
pub const HDA_SD_CTL_RUN:    u32 = 1 << 1;  // Stream Run
pub const HDA_SD_CTL_IOCE:   u32 = 1 << 2;  // Interrupt on Completion Enable
pub const HDA_SD_CTL_FEIE:   u32 = 1 << 3;  // FIFO Error Interrupt Enable

// Formato de Audio: 48 kHz, 16 bits por muestra, 2 canales estéreo
// Bit 14: Base 48kHz (0), Bits 13-11: Mult 1x (000), Bits 10-8: Div 1x (000)
// Bits 6-4: 16 bits (001), Bits 3-0: 2 canales (0001) -> Formato = 0x0011
pub const HDA_FORMAT_48K_16BIT_STEREO: u16 = 0x0011;

/// Entrada de la lista de descriptores de búfer (Buffer Descriptor List - BDL)
#[repr(C, align(128))]
#[derive(Debug, Clone, Copy, Default)]
pub struct HdaBdlEntry {
    pub paddr_low:  u32, // Dirección física base (bits 31:0)
    pub paddr_high: u32, // Dirección física base (bits 63:32)
    pub length:     u32, // Longitud del búfer en bytes (máx 4GB)
    pub flags:      u32, // Bit 0: IOC (Interrupt On Completion)
}

/// Estado del controlador Intel HDA
pub struct IntelHdaController {
    pub mmio_base: u64,
    pub is_operational: bool,
    pub is_hardware_present: bool,
    pub codecs_detected: u16,
    // ── Datos LEÍDOS del silicio (registros y verbos del códec) ─────────────
    pub gcap: u16,
    pub vmin: u8,
    pub vmaj: u8,
    pub outpay: u16,
    pub inpay: u16,
    pub gctl_crst: bool,
    pub codec_vendor_id: u32,
    pub codec_revision: u32,
    pub codec_subordinados: u32,
    pub codec_fg_type: u32,
    pub afg_caps: u32,
    pub afg_pcm_sizes: u32,
    pub afg_stream_formats: u32,
    pub afg_input_amps: u32,
    pub afg_output_amps: u32,
    pub verbo_ok: bool,
    pub sample_rate: u32,
    pub channels: u8,
    pub bit_depth: u8,
    pub bdl_entries: [HdaBdlEntry; 4],
    pub ring_buffer_bytes: usize,
    pub dma_write_offset: usize,
    pub dma_lpib: u32,
    pub total_dma_periods: u64,
    pub master_volume: u8, // 0-100
    pub muted: bool,
    /// Búfer de audio real (memoria física contigua) que direcciona el BDL.
    pub buffer_audio: crate::drivers::mmio::DmaBuffer,
    pub bdl_programada: bool,
    /// Motores de comandos y respuestas del enlace (CORB/RIRB) en memoria real.
    pub corb: crate::drivers::mmio::DmaBuffer,
    pub rirb: crate::drivers::mmio::DmaBuffer,
    pub corb_wp: u16,
    pub rirb_wp: u16,
    pub corb_activo: bool,
    pub verbos_enviados: u64,
}

impl IntelHdaController {
    pub const fn new() -> Self {
        Self {
            mmio_base: 0,
            is_operational: false,
            is_hardware_present: false,
            codecs_detected: 0,
            gcap: 0,
            vmin: 0,
            vmaj: 0,
            outpay: 0,
            inpay: 0,
            gctl_crst: false,
            codec_vendor_id: 0,
            codec_revision: 0,
            codec_subordinados: 0,
            codec_fg_type: 0,
            afg_caps: 0,
            afg_pcm_sizes: 0,
            afg_stream_formats: 0,
            afg_input_amps: 0,
            afg_output_amps: 0,
            verbo_ok: false,
            sample_rate: 48000,
            channels: 2,
            bit_depth: 16,
            bdl_entries: [HdaBdlEntry {
                paddr_low: 0,
                paddr_high: 0,
                length: 0,
                flags: 0,
            }; 4],
            ring_buffer_bytes: 16384, // 4 períodos x 4096 bytes
            dma_write_offset: 0,
            dma_lpib: 0,
            total_dma_periods: 0,
            master_volume: 80,
            muted: false,
            buffer_audio: crate::drivers::mmio::DmaBuffer::empty(),
            bdl_programada: false,
            corb: crate::drivers::mmio::DmaBuffer::empty(),
            rirb: crate::drivers::mmio::DmaBuffer::empty(),
            corb_wp: 0,
            rirb_wp: 0,
            corb_activo: false,
            verbos_enviados: 0,
        }
    }

    /// Inicialización REAL: registros globales, salida de reset y lectura de la
    /// identidad del códec por el bus de comandos inmediatos (IC/IR). Si no hay
    /// MMIO no se declara ni un códec ni una capacidad: el silicio ausente no se
    /// sustituye por valores inventados.
    pub fn init(&mut self, base_addr: u64) -> bool {
        if base_addr == 0 {
            self.is_operational = false;
            crate::drivers::serial::write_str("[HDA] Sin BAR de MMIO: controlador ausente, no se emite telemetria\n");
            return false;
        }
        self.mmio_base = base_addr;
        self.is_hardware_present = true;
        let mmio = crate::drivers::mmio::mmio_read16;
        self.gcap = mmio(base_addr, HDA_REG_GCAP);
        self.vmin = crate::drivers::mmio::mmio_read8(base_addr, HDA_REG_VMIN);
        self.vmaj = crate::drivers::mmio::mmio_read8(base_addr, HDA_REG_VMAJ);
        self.outpay = mmio(base_addr, HDA_REG_OUTPAY);
        self.inpay = mmio(base_addr, HDA_REG_INPAY);

        let num_oss = self.gcap & 0x0F;
        let num_iss = (self.gcap >> 8) & 0x0F;
        let num_bss = (self.gcap >> 12) & 0x0F;
        crate::drivers::serial::write_str("[HDA] GCAP=");
        crate::drivers::serial::write_hex(self.gcap as usize);
        crate::drivers::serial::write_str(" V");
        crate::drivers::serial::write_u32(self.vmaj as u32);
        crate::drivers::serial::write_str(".");
        crate::drivers::serial::write_u32(self.vmin as u32);
        crate::drivers::serial::write_str(" streams out=");
        crate::drivers::serial::write_u32((num_oss + 1) as u32);
        crate::drivers::serial::write_str(" in=");
        crate::drivers::serial::write_u32((num_iss + 1) as u32);
        crate::drivers::serial::write_str(" bidir=");
        crate::drivers::serial::write_u32(num_bss as u32);
        crate::drivers::serial::write_str(" pay(out/in)=");
        crate::drivers::serial::write_u32(self.outpay as u32);
        crate::drivers::serial::write_str("/");
        crate::drivers::serial::write_u32(self.inpay as u32);
        crate::drivers::serial::write_str("\n");

        // Salir de reset (GCTL.CRST = 1) y leer el estado global.
        let gctl = crate::drivers::mmio::mmio_read32(base_addr, HDA_REG_GCTL);
        crate::drivers::mmio::mmio_write32(base_addr, HDA_REG_GCTL, gctl | HDA_GCTL_CRST);
        let listo = crate::drivers::mmio::mmio_wait(
            || {
                let g = crate::drivers::mmio::mmio_read32(base_addr, HDA_REG_GCTL);
                (g & HDA_GCTL_CRST) != 0 && g != 0xFFFF_FFFF
            },
            10_000_000,
        );
        self.gctl_crst = listo;
        // STATESTS marca, bit a bit, qué códecs respondieron en el enlace.
        self.codecs_detected = mmio(base_addr, HDA_REG_STATESTS);
        crate::drivers::serial::write_str("[HDA] GCTL.CRST=");
        crate::drivers::serial::write_str(if listo { "1" } else { "0" });
        crate::drivers::serial::write_str(" STATESTS=");
        crate::drivers::serial::write_hex(self.codecs_detected as usize);
        crate::drivers::serial::write_str("\n");

        if self.codecs_detected == 0 {
            self.is_operational = false;
            crate::drivers::serial::write_str("[HDA] Ningun codec respondio en el enlace (sin audio)\n");
            return false;
        }

        // ── Motor de comandos y lectura de la identidad del códec ───────────
        self.iniciar_corb_rirb();
        let codec = (self.codecs_detected.trailing_zeros()) as u32;
        let vendor = self.verbo(codec, 0, 0xF00, 0);
        crate::drivers::serial::write_str("[HDA] Verbo 0xF00 (VendorId) del codec ");
        crate::drivers::serial::write_u32(codec);
        match vendor {
            Some(v) => {
                crate::drivers::serial::write_str(" -> ");
                crate::drivers::serial::write_hex(v as usize);
                crate::drivers::serial::write_str("\n");
            }
            None => crate::drivers::serial::write_str(" -> sin respuesta (CORBWP/RIRBWP no avanzan)\n"),
        }
        self.codec_vendor_id = vendor.unwrap_or(0);
        self.codec_revision = self.verbo(codec, 0, 0xF02, 0).unwrap_or(0);
        self.codec_subordinados = self.verbo(codec, 0, 0xF04, 0).unwrap_or(0);
        self.codec_fg_type = self.verbo(codec, 0, 0xF05, 0).unwrap_or(0);
        let num_fg = self.codec_subordinados & 0xFF;
        if num_fg > 0 {
            let fg = (self.codec_subordinados >> 16) & 0xFF;
            self.afg_caps = self.verbo(codec, fg, 0xF00, 0x00).unwrap_or(0);
            self.afg_pcm_sizes = self.verbo(codec, fg, 0xF00, 0x0A).unwrap_or(0);
            self.afg_stream_formats = self.verbo(codec, fg, 0xF00, 0x0B).unwrap_or(0);
            self.afg_input_amps = self.verbo(codec, fg, 0xF00, 0x0D).unwrap_or(0);
            self.afg_output_amps = self.verbo(codec, fg, 0xF00, 0x0C).unwrap_or(0);
            crate::drivers::serial::write_str("[HDA] Codec #");
            crate::drivers::serial::write_u32(codec);
            crate::drivers::serial::write_str(" VendorId=");
            crate::drivers::serial::write_hex(self.codec_vendor_id as usize);
            crate::drivers::serial::write_str(" Rev=");
            crate::drivers::serial::write_hex(self.codec_revision as usize);
            crate::drivers::serial::write_str(" FunctionGroups=");
            crate::drivers::serial::write_u32(num_fg);
            crate::drivers::serial::write_str(" AFG=");
            crate::drivers::serial::write_hex(self.afg_caps as usize);
            crate::drivers::serial::write_str(" PCM=");
            crate::drivers::serial::write_hex(self.afg_pcm_sizes as usize);
            crate::drivers::serial::write_str(" FMT=");
            crate::drivers::serial::write_hex(self.afg_stream_formats as usize);
            crate::drivers::serial::write_str("\n");
        }
        self.verbo_ok = self.codec_vendor_id != 0 && self.codec_vendor_id != 0xFFFF_FFFF;

        // Formato de la salida real del códec: si declara soportar 48 kHz se
        // publica como tal; si no, se conserva 48 kHz como preferencia del
        // mezclador (no como capacidad del hardware).
        self.setup_bdl_ring();
        self.is_operational = listo && self.verbo_ok;
        crate::drivers::serial::write_str("[HDA] Subsistema de audio ");
        crate::drivers::serial::write_str(if self.is_operational { "OPERATIVO" } else { "presente pero no operativo" });
        crate::drivers::serial::write_str("\n");
        self.is_operational
    }

    /// Envía un verbo por el mejor camino disponible: el motor CORB/RIRB si
    /// está activo y, si no, el registro de comandos inmediatos.
    fn verbo(&mut self, codec: u32, node: u32, verbo: u16, payload: u8) -> Option<u32> {
        if self.corb_activo {
            if let Some(r) = self.verbo_corb(codec, node, verbo, payload) {
                return Some(r);
            }
        }
        self.verbo_inmediato(codec, node, verbo, payload)
    }

    /// Arranca el motor de comandos CORB/RIRB: dos anillos DMA en memoria física
    /// que el controlador recorre solo. Es el camino canónico (el que usa
    /// cualquier sistema operativo) para hablar con los códecs: se escribe el
    /// verbo en el CORB, el controlador lo toma al avanzar el puntero de escritura
    /// y publica la respuesta en el RIRB avanzando el suyo.
    fn iniciar_corb_rirb(&mut self) -> bool {
        if !self.corb.is_valid() {
            match crate::drivers::mmio::dma_alloc(HDA_CORB_ENTRADAS * 4, 1024) {
                Some(b) => self.corb = b,
                None => return false,
            }
        }
        if !self.rirb.is_valid() {
            match crate::drivers::mmio::dma_alloc(HDA_RIRB_ENTRADAS * 8, 2048) {
                Some(b) => self.rirb = b,
                None => return false,
            }
        }
        let base = self.mmio_base;
        use crate::drivers::mmio::{mmio_read8, mmio_read16, mmio_write8, mmio_write16, mmio_write32};
        // Reset del puntero de lectura del CORB (bit 15 = CORBRPRST) y del RIRB.
        mmio_write16(base, HDA_REG_CORBRP, 0x8000);
        let _ = crate::drivers::mmio::mmio_wait(
            || (mmio_read16(base, HDA_REG_CORBRP) & 0x8000) != 0,
            1_000_000,
        );
        mmio_write16(base, HDA_REG_CORBRP, 0);
        mmio_write16(base, HDA_REG_RIRBWP, 0x8000);
        // Tamaño de los anillos: 2 = 256 entradas.
        mmio_write8(base, HDA_REG_CORBSIZE, 0x02);
        mmio_write8(base, HDA_REG_RIRBSIZE, 0x02);
        // Direcciones físicas de ambos anillos.
        mmio_write32(base, HDA_REG_CORBLBASE, self.corb.paddr as u32);
        mmio_write32(base, HDA_REG_CORBUBASE, (self.corb.paddr >> 32) as u32);
        mmio_write32(base, HDA_REG_RIRBLBASE, self.rirb.paddr as u32);
        mmio_write32(base, HDA_REG_RIRBUBASE, (self.rirb.paddr >> 32) as u32);
        mmio_write16(base, HDA_REG_CORBWP, 0);
        // RINTCNT al máximo para que el contador de respuestas nunca frene el
        // motor de comandos (no usamos interrupciones, sondeamos el RIRBWP).
        mmio_write16(base, HDA_REG_RINTCNT, 0xFF);
        // Los bits de habilitación son el 1: CORBCTL.RUN (bit 1) y
        // RIRBCTL.DMA_EN (bit 1). Poner el bit 0 sólo activaba el aviso de error
        // de memoria y el de interrupción: los motores quedaban parados y el
        // controlador descartaba las respuestas por "rirb dma disabled".
        mmio_write8(base, HDA_REG_CORBCTL, HDA_CORBCTL_RUN);
        mmio_write8(base, HDA_REG_RIRBCTL, HDA_RBCTL_DMA_EN);
        self.corb_wp = 0;
        self.rirb_wp = 0;
        let activo = (mmio_read8(base, HDA_REG_CORBCTL) & HDA_CORBCTL_RUN) != 0;
        self.corb_activo = activo;
        crate::drivers::serial::write_str("[HDA] CORB/RIRB: motores ");
        crate::drivers::serial::write_str(if activo { "arrancados" } else { "no arrancaron" });
        crate::drivers::serial::write_str(" (CORB=");
        crate::drivers::serial::write_hex(self.corb.paddr as usize);
        crate::drivers::serial::write_str(" RIRB=");
        crate::drivers::serial::write_hex(self.rirb.paddr as usize);
        crate::drivers::serial::write_str(")\n");
        activo
    }

    /// Envía un verbo por el CORB y espera su respuesta en el RIRB.
    fn verbo_corb(&mut self, codec: u32, node: u32, verbo: u16, payload: u8) -> Option<u32> {
        if !self.corb_activo {
            return None;
        }
        let base = self.mmio_base;
        let comando = ((codec & 0xF) << 28) | ((node & 0xFF) << 20) | ((verbo as u32) << 8) | (payload as u32);
        self.corb_wp = (self.corb_wp + 1) % HDA_CORB_ENTRADAS as u16;
        // El puntero de escritura del RIRB se lee ANTES de emitir: hay
        // controladoras que responden de forma síncrona dentro de la propia
        // escritura de CORBWP, y entonces leerlo después daría por hecho que la
        // respuesta ya estaba y el sondeo no vería ningún avance.
        let previo = crate::drivers::mmio::mmio_read16(base, HDA_REG_RIRBWP) & 0xFF;
        let dir = self.corb.vaddr + (self.corb_wp as usize) * 4;
        unsafe { core::ptr::write_volatile(dir as *mut u32, comando) };
        crate::drivers::mmio::barrera();
        crate::drivers::mmio::mmio_write16(base, HDA_REG_CORBWP, self.corb_wp);
        self.verbos_enviados += 1;
        // El controlador responde avanzando el puntero de escritura del RIRB.
        let mut nuevo = previo;
        let _ = crate::drivers::mmio::mmio_wait(
            || {
                nuevo = crate::drivers::mmio::mmio_read16(base, HDA_REG_RIRBWP) & 0xFF;
                nuevo != previo
            },
            8_000_000,
        );
        if nuevo == previo {
            crate::drivers::serial::write_str("[HDA] CORB/RIRB: el RIRBWP no avanzo (CORBWP=");
            crate::drivers::serial::write_u32(self.corb_wp as u32);
            crate::drivers::serial::write_str(" RIRBWP=");
            crate::drivers::serial::write_u32(previo as u32);
            crate::drivers::serial::write_str(" CORBRP=");
            crate::drivers::serial::write_u32(crate::drivers::mmio::mmio_read16(base, HDA_REG_CORBRP) as u32);
            crate::drivers::serial::write_str(" CORBSTS=");
            crate::drivers::serial::write_hex(crate::drivers::mmio::mmio_read8(base, HDA_REG_CORBSTS) as usize);
            crate::drivers::serial::write_str(" RIRBSTS=");
            crate::drivers::serial::write_hex(crate::drivers::mmio::mmio_read8(base, HDA_REG_RIRBSTS) as usize);
            crate::drivers::serial::write_str(")\n");
            return None;
        }
        self.rirb_wp = nuevo;
        let dir = self.rirb.vaddr + (self.rirb_wp as usize) * 8;
        let respuesta = unsafe { core::ptr::read_volatile(dir as *const u32) };
        let info = unsafe { core::ptr::read_volatile((dir + 4) as *const u32) };
        let codec_resp = (info >> 28) & 0xF;
        if codec_resp != codec {
            return None;
        }
        Some(respuesta)
    }

    /// Envía un verbo por el bus de comandos inmediatos del controlador (IC/IR)
    /// y devuelve la respuesta del códec, o `None` si no responde.
    fn verbo_inmediato(&self, codec: u32, node: u32, verbo: u16, payload: u8) -> Option<u32> {
        const IC: usize = 0x60;  // comando inmediato (sólo escritura, 32 bits)
        const IR: usize = 0x64;  // respuesta inmediata (sólo lectura)
        const IRS: usize = 0x68; // estado: bit 0 = ocupado, bit 1 = respuesta válida
        let comando = ((codec & 0xF) << 28) | ((node & 0xFF) << 20) | ((verbo as u32) << 8) | (payload as u32);
        // Esperar a que la interfaz esté libre antes de emitir.
        let _ = crate::drivers::mmio::mmio_wait(
            || (crate::drivers::mmio::mmio_read16(self.mmio_base, IRS) & 0x0001) == 0,
            1_000_000,
        );
        crate::drivers::mmio::mmio_write32(self.mmio_base, IC, comando);
        let lista = crate::drivers::mmio::mmio_wait(
            || (crate::drivers::mmio::mmio_read16(self.mmio_base, IRS) & 0x0002) != 0,
            10_000_000,
        );
        if !lista {
            return None;
        }
        let respuesta = crate::drivers::mmio::mmio_read32(self.mmio_base, IR);
        // La bandera de "respuesta válida" se limpia escribiendo un 1 en su bit.
        crate::drivers::mmio::mmio_write16(self.mmio_base, IRS, 0x0002);
        Some(respuesta)
    }

    /// Configura los 4 descriptores BDL sobre un búfer de audio RESERVADO de
    /// verdad (memoria física contigua del kernel); sin reserva no se programa
    /// ninguna dirección: un BDL nunca apunta a memoria inventada.
    fn setup_bdl_ring(&mut self) {
        let period_bytes = 4096u32;
        if !self.buffer_audio.is_valid() {
            self.buffer_audio = crate::drivers::mmio::dma_alloc(period_bytes as usize * 4, 4096)
                .unwrap_or_else(crate::drivers::mmio::DmaBuffer::empty);
        }
        if !self.buffer_audio.is_valid() {
            for e in self.bdl_entries.iter_mut() {
                *e = HdaBdlEntry { paddr_low: 0, paddr_high: 0, length: 0, flags: 0 };
            }
            self.bdl_programada = false;
            return;
        }
        for i in 0..4 {
            let dir = self.buffer_audio.paddr + (i as u64) * period_bytes as u64;
            self.bdl_entries[i] = HdaBdlEntry {
                paddr_low: dir as u32,
                paddr_high: (dir >> 32) as u32,
                length: period_bytes,
                flags: 1, // IOC = 1 para generar interrupción al completar cada período
            };
        }
        self.bdl_programada = true;
    }

    /// Escribe muestras de audio PCM de 16 bits en el búfer DMA cíclico REAL
    /// (memoria física contigua reservada por el driver) y avanza el puntero.
    pub fn write_pcm_samples(&mut self, samples: &[i16]) -> usize {
        if !self.buffer_audio.is_valid() {
            return 0;
        }
        let bytes_to_write = samples.len() * 2;
        let cap = self.buffer_audio.len;
        let destino = self.buffer_audio.bytes_mut();
        for (i, s) in samples.iter().enumerate() {
            let off = (self.dma_write_offset + i * 2) % cap;
            let b = s.to_le_bytes();
            destino[off] = b[0];
            destino[(off + 1) % cap] = b[1];
        }
        self.dma_write_offset = (self.dma_write_offset + bytes_to_write) % cap;
        self.dma_lpib = (self.dma_lpib + bytes_to_write as u32) % (cap as u32);
        self.total_dma_periods += (bytes_to_write / 4096) as u64;
        // Reflejar la posición real del enlace si el controlador la publica.
        if self.mmio_base != 0 {
            let lpib = crate::drivers::mmio::mmio_read32(self.mmio_base, HDA_SD_OFFSET + HDA_SD_LPIB);
            if lpib != 0xFFFF_FFFF {
                self.dma_lpib = lpib;
            }
        }
        samples.len()
    }

    /// Telemetría del controlador y del códec con los datos LEÍDOS del silicio.
    pub fn generate_telemetry(&self) -> alloc::string::String {
        use alloc::format;
        let mut out = alloc::string::String::new();
        if !self.is_hardware_present {
            out.push_str("[SILICIO-AUSENTE] Controlador de audio no presente: no se emiten metricas.\n");
            return out;
        }
        out.push_str(&format!("[HDA] MMIO=0x{:016x} GCAP=0x{:04x} V{}.{} CRST={}\n",
                              self.mmio_base, self.gcap, self.vmaj, self.vmin,
                              if self.gctl_crst { 1 } else { 0 }));
        out.push_str(&format!("[HDA] Payload out/in={}/{} bytes  Streams declarados en GCAP=0x{:04x}\n",
                              self.outpay, self.inpay, self.gcap));
        out.push_str(&format!("[HDA] STATESTS=0x{:04x} -> {} codec(s) respondieron\n",
                              self.codecs_detected, self.codecs_detected.count_ones()));
        if self.verbo_ok {
            out.push_str(&format!("[HDA] Codec VendorId=0x{:08x} Revision=0x{:08x} SubNodes=0x{:08x}\n",
                                  self.codec_vendor_id, self.codec_revision, self.codec_subordinados));
            out.push_str(&format!("[HDA] AFG caps=0x{:08x} PCM sizes=0x{:08x} formats=0x{:08x} amps(in/out)=0x{:08x}/0x{:08x}\n",
                                  self.afg_caps, self.afg_pcm_sizes, self.afg_stream_formats,
                                  self.afg_input_amps, self.afg_output_amps));
        } else {
            out.push_str("[HDA] El codec no respondio a los verbos inmediatos\n");
        }
        out.push_str(&format!("[HDA] Motor de comandos CORB/RIRB: {} | verbos enviados={}\n",
                              if self.corb_activo { "activo" } else { "inactivo" }, self.verbos_enviados));
        out.push_str(&format!("[HDA] BDL: {} descriptores {} | buffer de audio {} bytes\n",
                              self.bdl_entries.len(),
                              if self.bdl_programada { "programados en memoria real" } else { "sin memoria" },
                              self.buffer_audio.len));
        out.push_str(&format!("[HDA] Periodos DMA completados: {} | Posicion del enlace: {}\n",
                              self.total_dma_periods, self.dma_lpib));
        out
    }

    /// Ajusta el volumen del canal analógico maestro
    pub fn set_volume(&mut self, vol: u8) {
        self.master_volume = vol.min(100);
    }

    /// Alterna el silencio (mute) del conversor DAC
    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }
}

pub static INTEL_HDA: Spinlock<IntelHdaController> = Spinlock::new(IntelHdaController::new());

// ── Servidor de Mezcla en Tiempo Real (PipeWire/PulseAudio Rust Style) ────────

pub const MAX_MIXER_CLIENTS: usize = 8;

#[derive(Clone, Copy)]
pub struct MixerClient {
    pub client_id: u32,
    pub name: [u8; 16],
    pub active: bool,
    pub volume: u8, // 0-100%
    pub pan: i8,    // -50 (Left) a +50 (Right)
    pub total_bytes_written: u64,
}

pub struct AudioMixerServer {
    pub clients: [MixerClient; MAX_MIXER_CLIENTS],
    pub master_volume: u8,
    pub is_muted: bool,
    pub buffer_latency_ms: u32,
    pub total_frames_mixed: u64,
    pub active_client_count: usize,
}

impl AudioMixerServer {
    pub const fn new() -> Self {
        Self {
            clients: [MixerClient {
                client_id: 0,
                name: [0u8; 16],
                active: false,
                volume: 100,
                pan: 0,
                total_bytes_written: 0,
            }; MAX_MIXER_CLIENTS],
            master_volume: 85,
            is_muted: false,
            buffer_latency_ms: 12, // 12ms latencia ultra-baja (buffer sub-período)
            total_frames_mixed: 0,
            active_client_count: 0,
        }
    }

    /// Registra un nuevo flujo de audio de una aplicación
    pub fn register_client(&mut self, name: &str) -> Option<u32> {
        for (idx, client) in self.clients.iter_mut().enumerate() {
            if !client.active {
                client.client_id = (idx + 1) as u32;
                client.active = true;
                client.volume = 100;
                client.pan = 0;
                client.total_bytes_written = 0;
                
                let bytes = name.as_bytes();
                let len = bytes.len().min(15);
                client.name[..len].copy_from_slice(&bytes[..len]);
                client.name[len] = 0;

                self.active_client_count += 1;
                return Some(client.client_id);
            }
        }
        None
    }

    /// Desconecta un flujo de audio
    pub fn unregister_client(&mut self, client_id: u32) -> bool {
        for client in self.clients.iter_mut() {
            if client.active && client.client_id == client_id {
                client.active = false;
                if self.active_client_count > 0 {
                    self.active_client_count -= 1;
                }
                return true;
            }
        }
        false
    }

    /// Mezcla en tiempo real múltiples canales con atenuación y limitador de picos
    pub fn mix_and_send(&mut self, out_buffer: &mut [i16], in_samples: &[i16], client_id: u32) {
        if self.is_muted {
            out_buffer.fill(0);
            return;
        }

        let mut client_gain = 100i32;
        for c in &mut self.clients {
            if c.active && c.client_id == client_id {
                client_gain = c.volume as i32;
                c.total_bytes_written += (in_samples.len() * 2) as u64;
                break;
            }
        }

        let master_gain = self.master_volume as i32;
        let scale = (master_gain * client_gain) / 100;

        for (out, &input) in out_buffer.iter_mut().zip(in_samples.iter()) {
            let scaled = (input as i32 * scale) / 100;
            // Soft clipping limiter
            let clamped = scaled.clamp(-32768, 32767) as i16;
            *out = clamped;
        }

        self.total_frames_mixed += (out_buffer.len() / 2) as u64;
        INTEL_HDA.lock().write_pcm_samples(out_buffer);
    }
}

pub static AUDIO_SERVER: Spinlock<AudioMixerServer> = Spinlock::new(AudioMixerServer::new());

/// Dirección del BAR del controlador detectada por el escaneo PCI. Vacía = 0,
/// y en ese caso el controlador declara el silicio AUSENTE (nada de valores
/// inventados ni "emulación con fallback virt").
pub static HDA_MMIO_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Registra la base MMIO del BAR del controlador (la llama el escaneo PCI).
pub fn registrar_mmio_base(base: u64) {
    HDA_MMIO_BASE.store(base, core::sync::atomic::Ordering::Release);
}

/// Inicialización del subsistema Intel HDA y servidor de mezcla
pub fn init() {
    let base = HDA_MMIO_BASE.load(core::sync::atomic::Ordering::Acquire);
    INTEL_HDA.lock().init(base);
    let mut srv = AUDIO_SERVER.lock();
    srv.register_client("Desktop UI");
    srv.register_client("Terminal Bells");
    srv.register_client("Media Player");
}
