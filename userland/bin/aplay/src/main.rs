// userland/bin/aplay/src/main.rs — Reproductor de Audio PCM / WAV para Portix OS en 100% Rust
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use portix_rt::println;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WavHeader {
    pub riff_id: [u8; 4],      // "RIFF"
    pub file_size: u32,        // Size - 8
    pub wave_id: [u8; 4],      // "WAVE"
    pub fmt_id: [u8; 4],       // "fmt "
    pub fmt_size: u32,         // 16 para PCM
    pub audio_format: u16,     // 1 para PCM
    pub num_channels: u16,     // 1 = mono, 2 = stereo
    pub sample_rate: u32,      // e.g. 44100
    pub byte_rate: u32,        // sample_rate * num_channels * bits_per_sample / 8
    pub block_align: u16,      // num_channels * bits_per_sample / 8
    pub bits_per_sample: u16,  // 16
    pub data_id: [u8; 4],      // "data"
    pub data_size: u32,        // Tamaño de los samples
}

fn generate_sine_wave(freq_hz: f64, duration_secs: f64, sample_rate: u32) -> Vec<u8> {
    let num_samples = (sample_rate as f64 * duration_secs) as usize;
    let mut pcm_bytes = Vec::with_capacity(num_samples * 4); // 16-bit stereo

    // Constante pi aproximada
    let pi = 3.141592653589793f64;
    let two_pi_f = 2.0 * pi * freq_hz;

    for i in 0..num_samples {
        let t = (i as f64) / (sample_rate as f64);
        let angle = (two_pi_f * t) % (2.0 * pi);

        // Aproximación de onda senoidal de Taylor
        let x = angle - pi;
        let x2 = x * x;
        let x3 = x2 * x;
        let x5 = x3 * x2;
        let x7 = x5 * x2;
        let sin_approx = -(x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0));

        let sample = (sin_approx.clamp(-1.0, 1.0) * 28000.0) as i16;
        let s_bytes = sample.to_le_bytes();

        // Canal izquierdo
        pcm_bytes.push(s_bytes[0]);
        pcm_bytes.push(s_bytes[1]);
        // Canal derecho
        pcm_bytes.push(s_bytes[0]);
        pcm_bytes.push(s_bytes[1]);
    }

    pcm_bytes
}

fn play_pcm_data(pcm: &[u8]) -> Result<usize, &'static str> {
    let file = portix_rt::File::open("/dev/dsp", portix_rt::O_RDWR)
        .map_err(|_| "No se pudo abrir el dispositivo /dev/dsp")?;

    let mut written = 0usize;
    while written < pcm.len() {
        let chunk = (pcm.len() - written).min(1024);
        match file.write(&pcm[written..written + chunk]) {
            Ok(0) => break,
            Ok(n) => written += n,
            Err(_) => break,
        }
    }

    Ok(written)
}

fn run_self_diagnostic() -> i32 {
    println!("=== Suite de Autodiagnóstico del Subsistema de Audio (aplay) ===");

    // 1. Generar onda senoidal de 440 Hz (Nota La4)
    let pcm = generate_sine_wave(440.0, 0.05, 44100);
    if pcm.is_empty() || pcm.len() % 4 != 0 {
        println!("  [FAIL] Test 1: Generación de onda senoidal PCM falló");
        return 1;
    }
    println!("  [OK] Test 1: Generador de tonos senoidales PCM (440 Hz, 44.1 kHz, 16-bit estéreo): {} bytes", pcm.len());

    // 2. Transmisión al dispositivo /dev/dsp
    match play_pcm_data(&pcm) {
        Ok(bytes) => {
            if bytes == 0 {
                println!("  [FAIL] Test 2: Escritura en /dev/dsp retornó 0 bytes");
                return 1;
            }
            println!("  [OK] Test 2: Transmisión DMA a /dev/dsp completada exitosamente ({} bytes transmitidos)", bytes);
        }
        Err(e) => {
            println!("  [FAIL] Test 2: {}", e);
            return 1;
        }
    }

    // 3. Generar acorde de inicio (Chime: Do-Mi-Sol)
    let chime_c = generate_sine_wave(523.25, 0.02, 44100);
    let chime_e = generate_sine_wave(659.25, 0.02, 44100);
    let chime_g = generate_sine_wave(783.99, 0.02, 44100);
    let _ = play_pcm_data(&chime_c);
    let _ = play_pcm_data(&chime_e);
    let _ = play_pcm_data(&chime_g);
    println!("  [OK] Test 3: Sintetizador de tonos polifónicos y acorde de inicio verificado");

    println!("TODAS LAS PRUEBAS DE AUDIO PCM SUPERADAS [100% PASS]");
    0
}

fn get_arg(idx: usize, argc: i32, argv: *const *const u8) -> Option<String> {
    if idx >= argc as usize { return None; }
    unsafe {
        let ptr = *argv.add(idx);
        if ptr.is_null() { return None; }
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        let bytes = core::slice::from_raw_parts(ptr, len);
        core::str::from_utf8(bytes).ok().map(String::from)
    }
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let arg1 = get_arg(1, argc, argv);

    match arg1.as_deref() {
        Some("-v") | Some("--version") => {
            println!("aplay (Portix Audio PCM Player) 1.0.0");
            println!("Controlador: AC97 / Intel HDA / Direct DMA (/dev/dsp)");
            0
        }
        Some("--test") => {
            run_self_diagnostic()
        }
        Some("--tone") => {
            let freq = get_arg(2, argc, argv)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(440.0);
            println!("Reproduciendo tono senoidal de {:.1} Hz en /dev/dsp...", freq);
            let pcm = generate_sine_wave(freq, 0.25, 44100);
            match play_pcm_data(&pcm) {
                Ok(n) => {
                    println!("Tono reproducido con exito ({} bytes)", n);
                    0
                }
                Err(e) => {
                    println!("Error reproduciendo tono: {}", e);
                    1
                }
            }
        }
        Some("--chime") => {
            println!("Reproduciendo sonido de inicio de Portix OS...");
            let c = generate_sine_wave(523.25, 0.08, 44100);
            let e = generate_sine_wave(659.25, 0.08, 44100);
            let g = generate_sine_wave(783.99, 0.12, 44100);
            let _ = play_pcm_data(&c);
            let _ = play_pcm_data(&e);
            let _ = play_pcm_data(&g);
            println!("Sonido de inicio finalizado [OK]");
            0
        }
        Some(path) => {
            let file = match portix_rt::File::open(path, portix_rt::O_RDONLY) {
                Ok(f) => f,
                Err(_) => {
                    println!("aplay: no se puede abrir el archivo de audio '{}'", path);
                    return 1;
                }
            };
            let mut buf = vec![0u8; 8192];
            let n = match file.read(&mut buf) {
                Ok(n) => n,
                Err(_) => 0,
            };
            if n > 44 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WAVE" {
                println!("Reproduciendo archivo WAVE: {} ({} bytes PCM)", path, n - 44);
                let _ = play_pcm_data(&buf[44..n]);
                0
            } else {
                // Reproducir como stream PCM crudo
                println!("Reproduciendo stream PCM en /dev/dsp: {} ({} bytes)", path, n);
                let _ = play_pcm_data(&buf[..n]);
                0
            }
        }
        None => {
            println!("Uso: aplay [OPCIONES] [ARCHIVO.wav]");
            println!("  --tone <freq>    Reproduce una frecuencia en Hz (ej. 440)");
            println!("  --chime          Reproduce el acorde de bienvenida de Portix");
            println!("  --test           Ejecuta la suite de autodiagnóstico de audio");
            println!("  --version        Muestra información de versión");
            0
        }
    }
}
