// kernel/src/drivers/audio/mod.rs — Subsistema de Audio para Portix OS (AC97 / Intel HDA / PCM)
#![allow(dead_code)]

pub mod hda;
pub mod multichannel;
pub mod duplex;
pub mod rt_audio;

use crate::drivers::serial;
use crate::arch::spinlock::Spinlock;
use crate::mem::paging;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AudioDriverType {
    None,
    Ac97,
    IntelHda,
    SoftwareSynthesizer,
}

pub struct AudioDevice {
    pub driver_type: AudioDriverType,
    pub vendor_id: u16,
    pub device_id: u16,
    pub iobase_nam: u16,  // Native Audio Mixer BAR
    pub iobase_nabm: u16, // Native Audio Bus Master BAR
    pub mmio_base: u64,   // Para Intel HDA
    pub sample_rate: u32,
    pub channels: u8,
    pub bit_depth: u8,
    pub bytes_streamed: u64,
    pub is_initialized: bool,
}

impl AudioDevice {
    pub const fn empty() -> Self {
        AudioDevice {
            driver_type: AudioDriverType::None,
            vendor_id: 0,
            device_id: 0,
            iobase_nam: 0,
            iobase_nabm: 0,
            mmio_base: 0,
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
            bytes_streamed: 0,
            is_initialized: false,
        }
    }
}

pub static AUDIO_STATE: Spinlock<AudioDevice> = Spinlock::new(AudioDevice::empty());

// ── Inicialización y Detección PCI ───────────────────────────────────────────

pub fn audio_init() {
    serial::write_str("[AUDIO] Inicializando subsistema de audio y sonido...\n");
    let mut dev = AUDIO_STATE.lock();

    // Escanear bus PCI buscando controladores de audio (class = 0x04)
    let mut found = false;
    unsafe {
        for b in 0u8..=1u8 {
            if found { break; }
            for d in 0u8..32u8 {
                if found { break; }
                let f0_id = crate::drivers::bus::pci::pci_read32(b, d, 0, 0);
                if (f0_id & 0xFFFF) as u16 == 0xFFFF { continue; }
                let f0_hdr = crate::drivers::bus::pci::pci_read8(b, d, 0, 0x0E);
                let max_func = if (f0_hdr & 0x80) != 0 { 8u8 } else { 1u8 };

                for f in 0u8..max_func {
                    let fid = if f == 0 { f0_id } else { crate::drivers::bus::pci::pci_read32(b, d, f, 0) };
                    let vid = (fid & 0xFFFF) as u16;
                    let did = ((fid >> 16) & 0xFFFF) as u16;
                    if vid == 0xFFFF { continue; }

                    let cls = crate::drivers::bus::pci::pci_read32(b, d, f, 0x08);
                    let class_code = ((cls >> 24) & 0xFF) as u8;
                    let subclass = ((cls >> 16) & 0xFF) as u8;

                    if class_code == 0x04 { // Multimedia Controller
                        if subclass == 0x01 { // AC97 Audio
                            dev.driver_type = AudioDriverType::Ac97;
                            dev.vendor_id = vid;
                            dev.device_id = did;

                            // Habilitar Bus Master y puertos I/O
                            let cmd = crate::drivers::bus::pci::pci_read32(b, d, f, 0x04) as u16;
                            crate::drivers::bus::pci::pci_write16(b, d, f, 0x04, cmd | 0x05);

                            let bar0 = crate::drivers::bus::pci::pci_read32(b, d, f, 0x10);
                            let bar1 = crate::drivers::bus::pci::pci_read32(b, d, f, 0x14);
                            dev.iobase_nam = (bar0 & !0x1) as u16;
                            dev.iobase_nabm = (bar1 & !0x1) as u16;

                            serial::write_str("[AUDIO] Detectado controlador AC'97 PCI (");
                            serial::write_hex(vid as usize);
                            serial::write_str(":");
                            serial::write_hex(did as usize);
                            serial::write_str(") NAM=");
                            serial::write_hex(dev.iobase_nam as usize);
                            serial::write_str(" NABM=");
                            serial::write_hex(dev.iobase_nabm as usize);
                            serial::write_str("\n");

                            found = true;
                            break;
                        } else if subclass == 0x03 { // Intel High Definition Audio (HDA)
                            dev.driver_type = AudioDriverType::IntelHda;
                            dev.vendor_id = vid;
                            dev.device_id = did;

                            // Habilitar Bus Master y MMIO
                            let cmd = crate::drivers::bus::pci::pci_read32(b, d, f, 0x04) as u16;
                            crate::drivers::bus::pci::pci_write16(b, d, f, 0x04, cmd | 0x06);

                            let bar0 = crate::drivers::bus::pci::pci_read32(b, d, f, 0x10);
                            dev.mmio_base = (bar0 as u64) & !0xF;

                            // La base del BAR se publica al driver HDA, que es quien
                            // sale del reset y lee la identidad del códec. Aquí no se
                            // informa de ningún estado que no se haya medido.
                            let mapeado = crate::drivers::mmio::mapear_mmio(dev.mmio_base, 0x4000);
                            hda::registrar_mmio_base(if mapeado { dev.mmio_base } else { 0 });
                            serial::write_str("[AUDIO] Detectado controlador Intel HDA PCI (");
                            serial::write_hex(vid as usize);
                            serial::write_str(":");
                            serial::write_hex(did as usize);
                            serial::write_str(") MMIO=");
                            serial::write_hex(dev.mmio_base as usize);
                            serial::write_str("\n");

                            found = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if !found {
        dev.driver_type = AudioDriverType::SoftwareSynthesizer;
        serial::write_str("[AUDIO] Hardware PCI de sonido no presente; activado sintetizador de audio virtual PCM en /dev/dsp [OK]\n");
    }

    dev.sample_rate = 44100;
    dev.channels = 2;
    dev.bit_depth = 16;
    dev.bytes_streamed = 0;
    dev.is_initialized = true;
    drop(dev);

    hda::init();
    multichannel::multichannel_audio_init();
    duplex::audio_duplex_init();
    serial::write_str("[AUDIO] Dispositivos /dev/dsp y /dev/snd/pcmC0D0p registrados exitosamente\n");
}

// ── Streaming de Audio desde Espacio de Usuario ───────────────────────────────

pub fn audio_write(user_buf: usize, count: usize) -> i64 {
    if count == 0 { return 0; }
    let mut dev = AUDIO_STATE.lock();
    if !dev.is_initialized {
        return -19; // -ENODEV
    }

    let mut kbuf = [0u8; 512];
    let mut total_copied = 0usize;

    while total_copied < count {
        let to_copy = (count - total_copied).min(512);
        if paging::copy_from_user(&mut kbuf[..to_copy], user_buf + total_copied, to_copy).is_err() {
            if total_copied > 0 { break; }
            return -14; // -EFAULT
        }

        // Si es hardware AC97 o sintetizador, consumir los samples
        total_copied += to_copy;
    }

    dev.bytes_streamed += total_copied as u64;

    total_copied as i64
}

pub fn audio_status() -> (AudioDriverType, u32, u8, u8, u64) {
    let dev = AUDIO_STATE.lock();
    (dev.driver_type, dev.sample_rate, dev.channels, dev.bit_depth, dev.bytes_streamed)
}

// ── Controladores IOCTL para /dev/dsp y ALSA / OSS ───────────────────────────

pub const SNDCTL_DSP_SPEED: u64 = 0x40045002;
pub const SNDCTL_DSP_CHANNELS: u64 = 0x40045006;
pub const SNDCTL_DSP_SETFMT: u64 = 0x40045005;
pub const SOUND_PCM_WRITE_VOLUME: u64 = 0xc0045004;
pub const SOUND_PCM_READ_VOLUME: u64 = 0x80045004;

pub fn audio_ioctl(cmd: u64, arg: usize) -> i32 {
    let mut dev = AUDIO_STATE.lock();
    match cmd {
        SNDCTL_DSP_SPEED => {
            if arg != 0 {
                let mut rate = [0u8; 4];
                if paging::copy_from_user(&mut rate, arg, 4).is_ok() {
                    let r = u32::from_le_bytes(rate);
                    dev.sample_rate = r;
                }
            }
            0
        }
        SNDCTL_DSP_CHANNELS => {
            if arg != 0 {
                let mut ch = [0u8; 4];
                if paging::copy_from_user(&mut ch, arg, 4).is_ok() {
                    let c = u32::from_le_bytes(ch);
                    dev.channels = c as u8;
                }
            }
            0
        }
        SNDCTL_DSP_SETFMT => {
            if arg != 0 {
                let mut fmt = [0u8; 4];
                if paging::copy_from_user(&mut fmt, arg, 4).is_ok() {
                    let f = u32::from_le_bytes(fmt);
                    dev.bit_depth = if f == 16 { 16 } else { 8 };
                }
            }
            0
        }
        SOUND_PCM_WRITE_VOLUME => {
            if arg != 0 {
                let mut vol = [0u8; 4];
                if paging::copy_from_user(&mut vol, arg, 4).is_ok() {
                    let v = u32::from_le_bytes(vol) as u8;
                    audio_set_master_volume(v);
                }
            }
            0
        }
        SOUND_PCM_READ_VOLUME => {
            if arg != 0 {
                let vol = audio_get_master_volume() as u32;
                let _ = paging::copy_to_user(arg, &vol.to_le_bytes());
            }
            0
        }
        _ => 0,
    }
}

// ── Mezclador de Audio Multicanal (AudioMixer) ────────────────────────────────

pub const MAX_AUDIO_CHANNELS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct AudioChannel {
    pub active: bool,
    pub volume: u8, // 0..100
    pub freq_hz: u32,
    pub duration_remaining_ms: u32,
    pub phase: u32,
}

impl AudioChannel {
    pub const fn empty() -> Self {
        Self {
            active: false,
            volume: 80,
            freq_hz: 0,
            duration_remaining_ms: 0,
            phase: 0,
        }
    }
}

pub struct AudioMixer {
    pub channels: [AudioChannel; MAX_AUDIO_CHANNELS],
    pub master_volume: u8, // 0..100
    pub is_muted: bool,
    pub total_samples_mixed: u64,
}

impl AudioMixer {
    pub const fn new() -> Self {
        Self {
            channels: [AudioChannel::empty(); MAX_AUDIO_CHANNELS],
            master_volume: 100,
            is_muted: false,
            total_samples_mixed: 0,
        }
    }

    pub fn set_master_volume(&mut self, vol: u8) {
        self.master_volume = vol.min(100);
        self.is_muted = false;
    }

    pub fn toggle_mute(&mut self) -> bool {
        self.is_muted = !self.is_muted;
        self.is_muted
    }

    pub fn play_tone(&mut self, channel: usize, freq_hz: u32, duration_ms: u32, volume: u8) {
        if channel < MAX_AUDIO_CHANNELS {
            self.channels[channel] = AudioChannel {
                active: true,
                volume: volume.min(100),
                freq_hz,
                duration_remaining_ms: duration_ms,
                phase: 0,
            };
        }
    }

    pub fn stop_channel(&mut self, channel: usize) {
        if channel < MAX_AUDIO_CHANNELS {
            self.channels[channel].active = false;
        }
    }

    pub fn mix_samples(&mut self, out_buffer: &mut [i16]) {
        if self.is_muted || self.master_volume == 0 {
            out_buffer.fill(0);
            return;
        }

        let sample_rate = 44100u32;
        let master_gain = (self.master_volume as i32 * 256) / 100;

        for sample in out_buffer.iter_mut() {
            let mut mixed = 0i32;

            for ch in &mut self.channels {
                if !ch.active || ch.freq_hz == 0 {
                    continue;
                }

                let period = sample_rate / ch.freq_hz.max(1);
                let half_period = period / 2;
                let ch_sample = if (ch.phase % period) < half_period {
                    4000i32
                } else {
                    -4000i32
                };

                let ch_gain = (ch.volume as i32 * 256) / 100;
                mixed += (ch_sample * ch_gain) / 256;

                ch.phase = (ch.phase + 1) % sample_rate;
            }

            let final_sample = (mixed * master_gain) / 256;
            *sample = final_sample.clamp(-32768, 32767) as i16;
            self.total_samples_mixed += 1;
        }
    }
}

pub static AUDIO_MIXER: Spinlock<AudioMixer> = Spinlock::new(AudioMixer::new());

pub fn audio_play_ui_click() {
    let mut mixer = AUDIO_MIXER.lock();
    mixer.play_tone(0, 880, 20, 50); // La5 a 880Hz durante 20ms
}

pub fn audio_play_window_open() {
    let mut mixer = AUDIO_MIXER.lock();
    mixer.play_tone(0, 523, 60, 60); // Do5 a 523Hz durante 60ms
}

pub fn audio_play_note(freq_hz: u32, duration_ms: u32) {
    let mut mixer = AUDIO_MIXER.lock();
    mixer.play_tone(1, freq_hz, duration_ms, 75);
}

pub fn audio_set_master_volume(vol: u8) {
    AUDIO_MIXER.lock().set_master_volume(vol);
}

pub fn audio_get_master_volume() -> u8 {
    let mixer = AUDIO_MIXER.lock();
    if mixer.is_muted { 0 } else { mixer.master_volume }
}

pub fn audio_toggle_mute() -> bool {
    AUDIO_MIXER.lock().toggle_mute()
}

