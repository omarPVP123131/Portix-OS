// kernel/src/drivers/input/keyboard.rs
#![allow(dead_code)]

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") port,
                     options(nostack, nomem));
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up, Down, Left, Right,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Delete, Home, End, PageUp, PageDown, Insert,
}

pub struct KeyboardState {
    shift_l:  bool,
    shift_r:  bool,
    caps:     bool,
    numlock:  bool,
    ctrl_l:   bool,
    ctrl_r:   bool,
    alt_l:    bool,
    alt_r:    bool,
    e0_seen:  bool,
    e1_seen:  u8,   // para Pause/Break (E1 secuencia)
}

impl KeyboardState {
    pub const fn new() -> Self {
        KeyboardState {
            shift_l: false, shift_r: false,
            caps:    false,
            numlock: true,
            ctrl_l:  false, ctrl_r: false,
            alt_l:   false, alt_r: false,
            e0_seen: false,
            e1_seen: 0,
        }
    }

    #[inline(always)] pub fn ctrl(&self) -> bool { self.ctrl_l || self.ctrl_r }
    #[inline(always)] pub fn alt(&self)  -> bool { self.alt_l  || self.alt_r  }
    #[inline(always)] pub fn shift(&self)-> bool { self.shift_l || self.shift_r }
    #[inline(always)] pub fn num_lock(&self)-> bool { self.numlock }

    pub fn feed_byte(&mut self, sc: u8) -> Option<Key> {
        self.decode(sc)
    }

    fn decode(&mut self, sc: u8) -> Option<Key> {
        // ── Secuencia E1 (Pause/Break — ignorar) ────────────────────────
        if self.e1_seen > 0 {
            self.e1_seen -= 1;
            return None;
        }
        if sc == 0xE1 {
            self.e1_seen = 5; // E1 1D 45 E1 9D C5 — consumir los 5 bytes restantes
            return None;
        }

        // ── Prefijo E0 ───────────────────────────────────────────────────
        if sc == 0xE0 {
            self.e0_seen = true;
            return None;
        }

        let e0 = self.e0_seen;
        self.e0_seen = false;

        // ── Break codes (tecla soltada) ──────────────────────────────────
        if sc & 0x80 != 0 {
            let base = sc & 0x7F;
            match (e0, base) {
                (false, 0x2A) => self.shift_l = false,
                (false, 0x36) => self.shift_r = false,
                (false, 0x1D) => self.ctrl_l  = false,
                (true,  0x1D) => self.ctrl_r  = false,
                (false, 0x38) => self.alt_l   = false,
                (true,  0x38) => self.alt_r   = false,
                _ => {}
            }
            return None;
        }

        // ── Make codes con prefijo E0 ────────────────────────────────────
        // SOLO las flechas y teclas de navegación llevan E0 en PS/2 Set 1
        if e0 {
            return match sc {
                0x48 => Some(Key::Up),
                0x50 => Some(Key::Down),
                0x4B => Some(Key::Left),
                0x4D => Some(Key::Right),
                0x47 => Some(Key::Home),
                0x4F => Some(Key::End),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x52 => Some(Key::Insert),
                0x53 => Some(Key::Delete),
                0x1C => Some(Key::Enter),       // teclado numérico Enter
                0x35 => Some(Key::Char(b'/')),  // teclado numérico /
                0x1D => { self.ctrl_r = true; None }
                0x38 => { self.alt_r  = true; None }
                // PrintScreen, etc. — ignorar
                _ => None,
            };
        }

        // ── Make codes normales (sin E0) ─────────────────────────────────
        match sc {
            // Modificadores
            0x2A => { self.shift_l = true; None }
            0x36 => { self.shift_r = true; None }
            0x1D => { self.ctrl_l  = true; None }
            0x38 => { self.alt_l   = true; None }
            0x3A => { self.caps = !self.caps; None }
            0x45 => { self.numlock = !self.numlock; None }

            // Teclas especiales
            0x01 => Some(Key::Escape),
            0x0E => Some(Key::Backspace),
            0x0F => Some(Key::Tab),
            0x1C => Some(Key::Enter),

            // Teclas de función
            0x3B => Some(Key::F1),  0x3C => Some(Key::F2),
            0x3D => Some(Key::F3),  0x3E => Some(Key::F4),
            0x3F => Some(Key::F5),  0x40 => Some(Key::F6),
            0x41 => Some(Key::F7),  0x42 => Some(Key::F8),
            0x43 => Some(Key::F9),  0x44 => Some(Key::F10),
            0x57 => Some(Key::F11), 0x58 => Some(Key::F12),

            // Teclado numérico SIN E0 → respetan NumLock
            // With NumLock ON: producen caracteres numéricos
            // With NumLock OFF: producen navegación (Home, Up, etc.)
            // Con E0 siempre producen navegación (manejado arriba)
            0x47 => if self.numlock { Some(Key::Char(b'7')) } else { Some(Key::Home) },
            0x48 => if self.numlock { Some(Key::Char(b'8')) } else { Some(Key::Up) },
            0x49 => if self.numlock { Some(Key::Char(b'9')) } else { Some(Key::PageUp) },
            0x4A => Some(Key::Char(b'-')),
            0x4B => if self.numlock { Some(Key::Char(b'4')) } else { Some(Key::Left) },
            0x4C => Some(Key::Char(b'5')),
            0x4D => if self.numlock { Some(Key::Char(b'6')) } else { Some(Key::Right) },
            0x4E => Some(Key::Char(b'+')),
            0x4F => if self.numlock { Some(Key::Char(b'1')) } else { Some(Key::End) },
            0x50 => if self.numlock { Some(Key::Char(b'2')) } else { Some(Key::Down) },
            0x51 => if self.numlock { Some(Key::Char(b'3')) } else { Some(Key::PageDown) },
            0x52 => if self.numlock { Some(Key::Char(b'0')) } else { Some(Key::Insert) },
            0x53 => if self.numlock { Some(Key::Char(b'.')) } else { Some(Key::Delete) },

            // Caracteres normales
            _ => {
                let ch = self.sc_to_char(sc);
                if ch != 0 { Some(Key::Char(ch)) } else { None }
            }
        }
    }

    fn sc_to_char(&self, sc: u8) -> u8 {
        let sh = self.shift_l || self.shift_r;
        let up = sh ^ self.caps;

        // Fila de números: scancodes 0x02–0x0D
        const NUMS_N: &[u8] = b"1234567890-=";
        const NUMS_S: &[u8] = b"!@#$%^&*()_+";
        if sc >= 0x02 && sc <= 0x0D {
            let i = (sc - 0x02) as usize;
            return if sh { NUMS_S[i] } else { NUMS_N[i] };
        }

        // Resto del teclado
        // Formato: (scancode, minúscula/normal, mayúscula/shift)
        const MAP: &[(u8, u8, u8)] = &[
            // Fila superior (QWERTY)
            (0x10, b'q', b'Q'), (0x11, b'w', b'W'), (0x12, b'e', b'E'),
            (0x13, b'r', b'R'), (0x14, b't', b'T'), (0x15, b'y', b'Y'),
            (0x16, b'u', b'U'), (0x17, b'i', b'I'), (0x18, b'o', b'O'),
            (0x19, b'p', b'P'), (0x1A, b'[', b'{'), (0x1B, b']', b'}'),
            // Fila media (ASDF)
            (0x1E, b'a', b'A'), (0x1F, b's', b'S'), (0x20, b'd', b'D'),
            (0x21, b'f', b'F'), (0x22, b'g', b'G'), (0x23, b'h', b'H'),
            (0x24, b'j', b'J'), (0x25, b'k', b'K'), (0x26, b'l', b'L'),
            (0x27, b';', b':'), (0x28, b'\'', b'"'),
            // Backslash / pipe (0x2B — entre Enter y la fila de letras en US)
            (0x2B, b'\\', b'|'),
            // Tilde/backtick (0x29 — arriba izquierda, antes del 1)
            (0x29, b'`', b'~'),
            // Fila inferior (ZXCV)
            (0x2C, b'z', b'Z'), (0x2D, b'x', b'X'), (0x2E, b'c', b'C'),
            (0x2F, b'v', b'V'), (0x30, b'b', b'B'), (0x31, b'n', b'N'),
            (0x32, b'm', b'M'), (0x33, b',', b'<'), (0x34, b'.', b'>'),
            (0x35, b'/', b'?'),
            // Espacio
            (0x39, b' ', b' '),
        ];

        for &(code, lo, hi) in MAP {
            if sc == code {
                return if lo.is_ascii_alphabetic() {
                    if up { hi } else { lo }
                } else {
                    if sh { hi } else { lo }
                };
            }
        }
        0
    }
}