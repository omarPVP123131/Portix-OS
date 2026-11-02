// kernel/src/drivers/silicon.rs — Ensayo de SILICIO REAL (verificación de drivers)
//
// Un driver no está terminado cuando compila: está terminado cuando se comprueba
// sobre el hardware. Este módulo ejecuta el ensayo de aceptación de los cuatro
// controladores bare-metal y publica el resultado en el registro del kernel con
// marcadores estables (los mismos que consume `scripts/silicon_probe.py`).
//
// Qué se comprueba, y por qué cada comprobación vale:
//
//   * NVMe — identidad LEÍDA del controlador (número de serie, modelo y firma
//     "PORTIX-NVME-SILICIO-LBA0" escrita por el HOST en el bloque 0 del disco) y
//     ensayo de ida y vuelta: el kernel escribe un patrón en el último bloque y
//     lo relee. Si el camino MMIO/DMA estuviera mal (direcciones físicas mal
//     calculadas, colas mal programadas), la firma no aparecería y el patrón no
//     volvería.
//   * AHCI/SATA — modelo y capacidad LBA48 del disco real, firma del host en el
//     bloque 0 e ida y vuelta por DMA (READ/WRITE DMA EXT).
//   * xHCI/USB — descriptor de dispositivo leído por transferencias de control
//     EP0 (VID:PID, clase), endpoints bulk del descriptor de configuración y,
//     si el dispositivo es de almacenamiento masivo, firma del host en el
//     bloque 0 leída por SCSI READ(10) sobre Bulk-Only Transport.
//   * HDA — códecs que responden en el enlace (STATESTS) y su identidad leída
//     por verbos inmediatos (Vendor/Revision/AFG).
//
// Y hay una comprobación NEGATIVA: arrancando sin los dispositivos
// (`silicon=False` en el runner), los drivers deben declarar el silicio ausente
// en lugar de informar de hardware inexistente.
#![allow(dead_code)]

use alloc::string::String;
use crate::drivers::serial;

/// Firma que el anfitrión escribe en el bloque 0 de cada disco de prueba.
pub const FIRMA_NVME: &[u8] = b"PORTIX-NVME-SILICIO-LBA0";
pub const FIRMA_SATA: &[u8] = b"PORTIX-AHCI-SATA-LBA0";
pub const FIRMA_USB: &[u8] = b"PORTIX-XHCI-USB-LBA0";

#[derive(Clone, Copy)]
pub struct ResultadoSilicio {
    pub nvme_presente: bool,
    pub nvme_operativo: bool,
    pub nvme_firma_ok: bool,
    pub nvme_round_trip: bool,
    pub sata_presente: bool,
    pub sata_disco: bool,
    pub sata_firma_ok: bool,
    pub sata_round_trip: bool,
    pub usb_presente: bool,
    pub usb_descriptor: bool,
    pub usb_mass_storage: bool,
    pub usb_firma_ok: bool,
    pub usb_round_trip: bool,
    pub hda_presente: bool,
    pub hda_codec: bool,
    pub nvme_capacidad: u64,
    pub sata_capacidad: u64,
    pub usb_capacidad: u64,
}

impl ResultadoSilicio {
    pub const fn vacio() -> Self {
        Self {
            nvme_presente: false, nvme_operativo: false, nvme_firma_ok: false, nvme_round_trip: false,
            sata_presente: false, sata_disco: false, sata_firma_ok: false, sata_round_trip: false,
            usb_presente: false, usb_descriptor: false, usb_mass_storage: false, usb_firma_ok: false,
            usb_round_trip: false, hda_presente: false, hda_codec: false,
            nvme_capacidad: 0, sata_capacidad: 0, usb_capacidad: 0,
        }
    }

    /// ¿Se ha ejecutado ya un ensayo? (distingue "aún no medido" de "ausente")
    pub const fn medido(&self) -> bool { self.nvme_presente || self.sata_presente || self.usb_presente || self.hda_presente }

    /// Veredicto estricto: el ensayo sólo se declara superado cuando los CUATRO
    /// controladores están presentes y además verificados con medidas reales
    /// (identidad leída del controlador, firma del anfitrión y ensayo de ida y
    /// vuelta). Antes bastaba con que "lo ausente no fallara", lo que convertía
    /// un arranque sin hardware en un PASS vacío: exactamente la telemetría
    /// falsa que este frente debe eliminar.
    pub fn todo_ok(&self) -> bool {
        let nvme_ok = self.nvme_presente && self.nvme_operativo && self.nvme_firma_ok && self.nvme_round_trip;
        let sata_ok = self.sata_presente && self.sata_disco && self.sata_firma_ok && self.sata_round_trip;
        let usb_ok = self.usb_presente && self.usb_descriptor && self.usb_mass_storage
            && self.usb_firma_ok && self.usb_round_trip;
        let hda_ok = self.hda_presente && self.hda_codec;
        nvme_ok && sata_ok && usb_ok && hda_ok
    }

    /// ¿Faltó absolutamente todo el silicio? (ningún controlador en el bus)
    pub const fn ausente_total(&self) -> bool { !self.medido() }

    /// Veredicto en una sola cadena, para que la consola nunca improvise:
    /// PASS sólo con los cuatro verificados, AUSENTE si no hay nada que medir.
    pub fn veredicto(&self) -> &'static str {
        if self.todo_ok() { "[SILICIO-PASS]" }
        else if self.ausente_total() { "[SILICIO-AUSENTE]" }
        else { "[SILICIO-FAIL]" }
    }
}

/// Ejecuta el ensayo completo y publica los marcadores en el registro del kernel.
pub fn ejecutar_ensayo() -> ResultadoSilicio {
    let mut r = ResultadoSilicio::vacio();
    serial::write_str("\n[SILICIO] ===== ENSAYO DE SILICIO REAL (drivers bare-metal) =====\n");

    // ── NVMe ────────────────────────────────────────────────────────────────
    {
        let mut nvme = crate::drivers::storage::nvme::GLOBAL_NVME.lock();
        nvme.probe_and_init();
        r.nvme_presente = nvme.is_hardware_present;
        r.nvme_operativo = nvme.is_initialized;
        r.nvme_capacidad = nvme.nsze_blocks;
        if r.nvme_operativo {
            let mut bloque = [0u8; 512];
            if nvme.read_lba0(&mut bloque) {
                r.nvme_firma_ok = bloque.starts_with(FIRMA_NVME);
                serial::write_str("[SILICIO-NVME-LBA0] bloque 0 del namespace leido por DMA: \"");
                escribir_ascii(&bloque[..FIRMA_NVME.len()]);
                serial::write_str("\" (firma del anfitrion=");
                serial::write_str(if r.nvme_firma_ok { "COINCIDE" } else { "NO COINCIDE" });
                serial::write_str(")\n");
            }
            r.nvme_round_trip = nvme.self_test();
            serial::write_str("[SILICIO-NVME-ROUNDTRIP] patron escrito y releido en el bloque ");
            serial::write_u64(nvme.round_trip_lba);
            serial::write_str(" -> ");
            serial::write_str(if r.nvme_round_trip { "COINCIDE" } else { "NO COINCIDE" });
            serial::write_str("\n");
        }
        serial::write_str("[SILICIO-NVME] presente=");
        serial::write_str(if r.nvme_presente { "SI" } else { "NO" });
        serial::write_str(" operativo=");
        serial::write_str(if r.nvme_operativo { "SI" } else { "NO" });
        serial::write_str(" bloques=");
        serial::write_u64(r.nvme_capacidad);
        serial::write_str(" errores=");
        serial::write_u64(nvme.errors);
        serial::write_str("\n");
    }

    // ── AHCI/SATA ───────────────────────────────────────────────────────────
    {
        let mut ahci = crate::drivers::storage::ahci::GLOBAL_AHCI.lock();
        r.sata_presente = ahci.is_hardware_present;
        if r.sata_presente && ahci.disco_activo >= 0 {
            let p = ahci.disco_activo as usize;
            r.sata_disco = true;
            r.sata_capacidad = ahci.puertos[p].sectores_lba48;
            let mut bloque = [0u8; 512];
            if ahci.read_lba0(&mut bloque) {
                r.sata_firma_ok = bloque.starts_with(FIRMA_SATA);
                serial::write_str("[SILICIO-SATA-LBA0] bloque 0 del disco leido por READ DMA EXT: \"");
                escribir_ascii(&bloque[..FIRMA_SATA.len()]);
                serial::write_str("\" (firma del anfitrion=");
                serial::write_str(if r.sata_firma_ok { "COINCIDE" } else { "NO COINCIDE" });
                serial::write_str(")\n");
            }
            r.sata_round_trip = ahci.self_test();
            serial::write_str("[SILICIO-SATA-ROUNDTRIP] patron escrito y releido en el bloque ");
            serial::write_u64(ahci.puertos[p].sectores_lba48.saturating_sub(1));
            serial::write_str(" -> ");
            serial::write_str(if r.sata_round_trip { "COINCIDE" } else { "NO COINCIDE" });
            serial::write_str("\n");
        }
        serial::write_str("[SILICIO-SATA] controlador=");
        serial::write_str(if r.sata_presente { "SI" } else { "NO" });
        serial::write_str(" disco=");
        serial::write_str(if r.sata_disco { "SI" } else { "NO" });
        serial::write_str(" comandos=");
        serial::write_u64(ahci.comandos_emitidos);
        serial::write_str(" timeouts=");
        serial::write_u64(ahci.timeouts);
        serial::write_str("\n");
    }

    // ── xHCI/USB ────────────────────────────────────────────────────────────
    {
        let mut usb = crate::drivers::bus::xhci::XHCI_DEV.lock();
        usb.probe_and_init();
        r.usb_presente = usb.is_hardware_present;
        r.usb_descriptor = usb.desc_device_ok;
        r.usb_mass_storage = usb.es_mass_storage;
        r.usb_capacidad = usb.capacidad_sectores;
        if r.usb_descriptor {
            serial::write_str("[SILICIO-USB-DESCRIPTOR] VID:PID=");
            serial::write_hex(usb.vid as usize);
            serial::write_str(":");
            serial::write_hex(usb.pid as usize);
            serial::write_str(" clase=");
            serial::write_hex(usb.clase as usize);
            serial::write_str("/");
            serial::write_hex(usb.subclase as usize);
            serial::write_str("/");
            serial::write_hex(usb.protocolo as usize);
            serial::write_str(" (leido por control EP0)\n");
        }
        if r.usb_mass_storage {
            let mut bloque = [0u8; 512];
            if usb.read_lba0(&mut bloque) {
                r.usb_firma_ok = bloque.starts_with(FIRMA_USB);
                serial::write_str("[SILICIO-USB-LBA0] bloque 0 leido por SCSI READ(10) sobre Bulk-Only: \"");
                escribir_ascii(&bloque[..FIRMA_USB.len()]);
                serial::write_str("\" (firma del anfitrion=");
                serial::write_str(if r.usb_firma_ok { "COINCIDE" } else { "NO COINCIDE" });
                serial::write_str(")\n");
            }
            r.usb_round_trip = usb.self_test();
            serial::write_str("[SILICIO-USB-ROUNDTRIP] patron escrito y releido en el bloque ");
            serial::write_u32(usb.round_trip_lba);
            serial::write_str(" -> ");
            serial::write_str(if r.usb_round_trip { "COINCIDE" } else { "NO COINCIDE" });
            serial::write_str("\n");
        }
        serial::write_str("[SILICIO-USB] controlador=");
        serial::write_str(if r.usb_presente { "SI" } else { "NO" });
        serial::write_str(" descriptor=");
        serial::write_str(if r.usb_descriptor { "SI" } else { "NO" });
        serial::write_str(" almacenamiento=");
        serial::write_str(if r.usb_mass_storage { "SI" } else { "NO" });
        serial::write_str(" puertos_conectados=");
        serial::write_u32(usb.connected_ports);
        serial::write_str("\n");
    }

    // ── HDA ─────────────────────────────────────────────────────────────────
    {
        let hda = crate::drivers::audio::hda::INTEL_HDA.lock();
        r.hda_presente = hda.is_hardware_present;
        r.hda_codec = hda.verbo_ok;
        serial::write_str("[SILICIO-HDA] controlador=");
        serial::write_str(if r.hda_presente { "SI" } else { "NO" });
        serial::write_str(" codecs=");
        serial::write_u32(hda.codecs_detected.count_ones());
        serial::write_str(" identidad_por_verbos=");
        serial::write_str(if r.hda_codec { "SI" } else { "NO" });
        serial::write_str(" vendor=");
        serial::write_hex(hda.codec_vendor_id as usize);
        serial::write_str("\n");
    }

    // ── Veredicto ───────────────────────────────────────────────────────────
    let todo = r.todo_ok();
    if todo {
        serial::write_str("[SILICIO-PASS] los cuatro controladores bare-metal operados sobre hardware real:\n");
        serial::write_str("               NVMe (IDENTIFY + DMA), AHCI/SATA (DMA EXT), xHCI (descriptor EP0) y HDA (verbos)\n");
    } else if r.ausente_total() {
        // Prueba negativa: sin dispositivos en el bus no se declara nada
        // operativo ni se inventan métricas. La suite exige esta línea cuando
        // arranca sin silicio, para que un PASS falso sea imposible.
        serial::write_str("[SILICIO-AUSENTE] sin controladores NVMe/SATA/USB/HDA en el bus: nada que medir\n");
    } else {
        serial::write_str("[SILICIO-FAIL] algun controlador PRESENTE no completo su ensayo (ver lineas anteriores)\n");
    }
    r
}

/// Informe completo del estado del silicio (para el comando `silicio`).
pub fn informe() -> String {
    let mut out = String::new();
    out.push_str("===== ESTADO DEL SILICIO (medido, no estimado) =====\n");
    out.push_str(&crate::drivers::storage::nvme::GLOBAL_NVME.lock().generate_telemetry());
    out.push('\n');
    out.push_str(&crate::drivers::storage::ahci::GLOBAL_AHCI.lock().generate_telemetry());
    out.push('\n');
    out.push_str(&crate::drivers::bus::xhci::XHCI_DEV.lock().generate_telemetry());
    out.push('\n');
    out.push_str(&crate::drivers::audio::hda::INTEL_HDA.lock().generate_telemetry());
    out
}

/// Último resultado del ensayo (publicado por el arranque y consultable por la
/// terminal y por `/proc/silicon`).
pub static ULTIMO_ENSAYO: crate::arch::Spinlock<ResultadoSilicio> =
    crate::arch::Spinlock::new(ResultadoSilicio::vacio());

pub fn registrar_resultado(r: ResultadoSilicio) {
    *ULTIMO_ENSAYO.lock() = r;
}

pub fn ultimo_resultado() -> ResultadoSilicio {
    *ULTIMO_ENSAYO.lock()
}

fn escribir_ascii(bytes: &[u8]) {
    for &b in bytes {
        if b >= 0x20 && b < 0x7F {
            serial::write_byte(b);
        } else {
            serial::write_str(".");
        }
    }
}
