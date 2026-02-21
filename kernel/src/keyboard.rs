// kernel/src/keyboard.rs - PORTIX PS/2 Keyboard Driver (polling, no IRQ needed)
// Soporta: letras, números, símbolos, shift/caps, flechas, F1-F10, especiales
#![allow(dead_code)]

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nostack, nomem));
    v
}

// ── Key enum ──────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(u8),   // ASCII imprimible
    Enter,
    Backspace,
    Tab,
    Escape,
    // Flechas
    Up, Down, Left, Right,
    // Funciones
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10,
    // Navegación
    Delete, Home, End, PageUp, PageDown, Insert,
}

// ── Keyboard state ────────────────────────────────────────────────────────────
pub struct KeyboardState {
    shift_l:  bool,
    shift_r:  bool,
    caps:     bool,
    ctrl:     bool,
    alt:      bool,
    e0_seen:  bool,  // prefijo de tecla extendida 0xE0
}

impl KeyboardState {
    pub const fn new() -> Self {
        KeyboardState {
            shift_l: false, shift_r: false,
            caps: false, ctrl: false, alt: false,
            e0_seen: false,
        }
    }

    #[inline(always)] pub fn ctrl(&self) -> bool { self.ctrl }
    #[inline(always)] pub fn alt(&self)  -> bool { self.alt  }

    /// Lee el buffer del controlador PS/2 y devuelve un Key si hay uno.
    /// Seguro de llamar en cualquier momento (polling sin IRQ).
    pub fn poll(&mut self) -> Option<Key> {
        unsafe {
            let st = inb(PS2_STATUS);
            // Bit 0: output buffer full; Bit 5: dato de ratón (no teclado)
            if st & 0x01 == 0  { return None; }
            if st & 0x20 != 0  { let _ = inb(PS2_DATA); return None; }
            let sc = inb(PS2_DATA);
            self.decode(sc)
        }
    }

    fn decode(&mut self, sc: u8) -> Option<Key> {
        // Prefijo tecla extendida
        if sc == 0xE0 { self.e0_seen = true; return None; }

        let e0  = self.e0_seen;
        self.e0_seen = false;

        // ── Break codes (tecla soltada, bit 7 a 1) ───────────────────────────
        if sc & 0x80 != 0 {
            match (e0, sc & 0x7F) {
                (false, 0x2A) => self.shift_l = false,
                (false, 0x36) => self.shift_r = false,
                (false, 0x1D) | (true, 0x1D) => self.ctrl = false,
                (false, 0x38) | (true, 0x38) => self.alt  = false,
                _ => {}
            }
            return None;
        }

        // ── Extended make codes ───────────────────────────────────────────────
        if e0 {
            return match sc {
                0x48 => Some(Key::Up),    0x50 => Some(Key::Down),
                0x4B => Some(Key::Left),  0x4D => Some(Key::Right),
                0x47 => Some(Key::Home),  0x4F => Some(Key::End),
                0x49 => Some(Key::PageUp),0x51 => Some(Key::PageDown),
                0x52 => Some(Key::Insert),0x53 => Some(Key::Delete),
                0x1D => { self.ctrl = true; None }
                0x38 => { self.alt  = true; None }
                _ => None,
            };
        }

        // ── Regular make codes ────────────────────────────────────────────────
        match sc {
            0x2A => { self.shift_l = true;      None }
            0x36 => { self.shift_r = true;      None }
            0x1D => { self.ctrl    = true;      None }
            0x38 => { self.alt     = true;      None }
            0x3A => { self.caps = !self.caps;   None }

            0x01 => Some(Key::Escape),
            0x0E => Some(Key::Backspace),
            0x0F => Some(Key::Tab),
            0x1C => Some(Key::Enter),

            0x48 => Some(Key::Up),    0x50 => Some(Key::Down),
            0x4B => Some(Key::Left),  0x4D => Some(Key::Right),

            0x3B => Some(Key::F1),  0x3C => Some(Key::F2),
            0x3D => Some(Key::F3),  0x3E => Some(Key::F4),
            0x3F => Some(Key::F5),  0x40 => Some(Key::F6),
            0x41 => Some(Key::F7),  0x42 => Some(Key::F8),
            0x43 => Some(Key::F9),  0x44 => Some(Key::F10),

            _ => {
                let ch = self.sc_to_char(sc);
                if ch != 0 { Some(Key::Char(ch)) } else { None }
            }
        }
    }

    fn sc_to_char(&self, sc: u8) -> u8 {
        let sh  = self.shift_l || self.shift_r;
        let up  = sh ^ self.caps; // uppercase para letras

        // ── Fila numérica ─────────────────────────────────────────────────────
        const NUMS_N: &[u8] = b"1234567890-=";
        const NUMS_S: &[u8] = b"!@#$%^&*()_+";
        if sc >= 0x02 && sc <= 0x0D {
            let i = (sc - 0x02) as usize;
            return if sh { NUMS_S[i] } else { NUMS_N[i] };
        }

        // ── Mapa QWERTY completo ──────────────────────────────────────────────
        // (scancode, normal, shifted/upper)
        const MAP: &[(u8, u8, u8)] = &[
            (0x10,b'q',b'Q'),(0x11,b'w',b'W'),(0x12,b'e',b'E'),(0x13,b'r',b'R'),
            (0x14,b't',b'T'),(0x15,b'y',b'Y'),(0x16,b'u',b'U'),(0x17,b'i',b'I'),
            (0x18,b'o',b'O'),(0x19,b'p',b'P'),(0x1A,b'[',b'{'),(0x1B,b']',b'}'),
            (0x1E,b'a',b'A'),(0x1F,b's',b'S'),(0x20,b'd',b'D'),(0x21,b'f',b'F'),
            (0x22,b'g',b'G'),(0x23,b'h',b'H'),(0x24,b'j',b'J'),(0x25,b'k',b'K'),
            (0x26,b'l',b'L'),(0x27,b';',b':'),(0x28,b'\'',b'"'),(0x29,b'`',b'~'),
            (0x2B,b'\\',b'|'),
            (0x2C,b'z',b'Z'),(0x2D,b'x',b'X'),(0x2E,b'c',b'C'),(0x2F,b'v',b'V'),
            (0x30,b'b',b'B'),(0x31,b'n',b'N'),(0x32,b'm',b'M'),
            (0x33,b',',b'<'),(0x34,b'.',b'>'),(0x35,b'/',b'?'),
            (0x39,b' ',b' '),
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