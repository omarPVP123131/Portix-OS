// kernel/src/mouse.rs — PORTIX PS/2 Mouse Driver (fixed)
#![allow(dead_code)]

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

#[inline(always)] unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack, nomem));
    v
}
#[inline(always)] unsafe fn outb(p: u16, v: u8) {
    core::arch::asm!("out dx, al", in("dx") p, in("al") v, options(nostack, nomem));
}

unsafe fn wait_write() {
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x02 != 0 && lim > 0 { lim -= 1; }
}
unsafe fn wait_read() -> bool {
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x01 == 0 && lim > 0 { lim -= 1; }
    inb(PS2_STATUS) & 0x01 != 0
}

/// Drain both KBC buffers (keyboard + mouse).
unsafe fn drain_kbc() {
    let mut lim = 1024u32;
    while inb(PS2_STATUS) & 0x01 != 0 && lim > 0 {
        let _ = inb(PS2_DATA);
        lim -= 1;
    }
}

pub struct MouseState {
    pub x:       i32,
    pub y:       i32,
    pub buttons: u8,
    pkt:         [u8; 3],
    pkt_idx:     u8,
    max_x:       i32,
    max_y:       i32,
    pub present: bool,
}

impl MouseState {
    pub const fn new() -> Self {
        MouseState { x:0,y:0,buttons:0, pkt:[0;3], pkt_idx:0,
                     max_x:1023, max_y:767, present:false }
    }

    pub fn init(&mut self, sw: usize, sh: usize) -> bool {
        self.max_x = sw as i32 - 1;
        self.max_y = sh as i32 - 1;
        self.x     = sw as i32 / 2;
        self.y     = sh as i32 / 2;

        unsafe {
            drain_kbc();

            // Enable aux port
            wait_write(); outb(PS2_CMD, 0xA8);

            // Read + patch config byte
            wait_write(); outb(PS2_CMD, 0x20);
            if !wait_read() { return false; }
            let cfg = inb(PS2_DATA);
            let new_cfg = (cfg | 0x02) & !0x20; // IRQ12 on, mouse clock on
            wait_write(); outb(PS2_CMD, 0x60);
            wait_write(); outb(PS2_DATA, new_cfg);

            // Reset mouse
            wait_write(); outb(PS2_CMD, 0xD4);
            wait_write(); outb(PS2_DATA, 0xFF);
            if !wait_read() { return false; }
            let ack = inb(PS2_DATA); if ack != 0xFA { return false; }
            // Read self-test result (0xAA) + device id (0x00)
            if wait_read() { let _ = inb(PS2_DATA); }
            if wait_read() { let _ = inb(PS2_DATA); }

            // Set defaults
            wait_write(); outb(PS2_CMD, 0xD4);
            wait_write(); outb(PS2_DATA, 0xF6); // set defaults
            if wait_read() { let _ = inb(PS2_DATA); } // ack

            // Enable data reporting
            wait_write(); outb(PS2_CMD, 0xD4);
            wait_write(); outb(PS2_DATA, 0xF4);
            if !wait_read() { return false; }
            let ack2 = inb(PS2_DATA);
            if ack2 != 0xFA { return false; }

            self.present = true;
            true
        }
    }

    /// Poll without IRQ. Returns true if position or buttons changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        unsafe {
            loop {
                let st = inb(PS2_STATUS);
                if st & 0x01 == 0 { break; }      // buffer empty
                if st & 0x20 == 0 {
                    // Keyboard data — consume and skip so mouse data isn't blocked
                    let _ = inb(PS2_DATA);
                    continue;
                }
                let byte = inb(PS2_DATA);
                let (new_pos, packet_done) = self.feed(byte);
                if packet_done { changed = new_pos; }
            }
        }
        changed
    }

    /// Returns (position_changed, packet_complete).
    fn feed(&mut self, byte: u8) -> (bool, bool) {
        match self.pkt_idx {
            0 => {
                if byte & 0x08 == 0 { return (false, false); } // sync
                self.pkt[0] = byte;
                self.pkt_idx = 1;
                (false, false)
            }
            1 => { self.pkt[1] = byte; self.pkt_idx = 2; (false, false) }
            2 => {
                self.pkt[2] = byte;
                self.pkt_idx = 0;
                let changed = self.process();
                (changed, true)
            }
            _ => { self.pkt_idx = 0; (false, false) }
        }
    }

    fn process(&mut self) -> bool {
        let flags = self.pkt[0];
        if flags & 0xC0 != 0 { return false; } // overflow
        let new_buttons = flags & 0x07;

        let raw_dx = self.pkt[1] as i32;
        let raw_dy = self.pkt[2] as i32;
        let dx = if flags & 0x10 != 0 { raw_dx | !0xFF } else { raw_dx };
        let dy = if flags & 0x20 != 0 { raw_dy | !0xFF } else { raw_dy };

        let new_x = (self.x + dx).max(0).min(self.max_x);
        let new_y = (self.y - dy).max(0).min(self.max_y); // Y inverted

        let moved = new_x != self.x || new_y != self.y || new_buttons != self.buttons;
        self.x = new_x; self.y = new_y; self.buttons = new_buttons;
        moved
    }

    #[inline] pub fn left_btn(&self)   -> bool { self.buttons & 0x01 != 0 }
    #[inline] pub fn right_btn(&self)  -> bool { self.buttons & 0x02 != 0 }
    #[inline] pub fn middle_btn(&self) -> bool { self.buttons & 0x04 != 0 }
}