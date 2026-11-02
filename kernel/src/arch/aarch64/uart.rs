// kernel/src/arch/aarch64/uart.rs — Driver de Puerto Serie PrimeCell PL011 UART
// Utilizado en plataformas ARM64: QEMU virt (0x0900_0000) y Raspberry Pi 4/5 (0xFE20_1000)
#![allow(dead_code)]

/// Registros MMIO del periférico ARM PrimeCell PL011 UART
#[repr(C)]
pub struct Pl011Registers {
    pub dr: u32,       // 0x000: Data Register
    pub rsr_ecr: u32,  // 0x004: Receive Status Register / Error Clear
    _reserved1: [u32; 4],
    pub fr: u32,       // 0x018: Flag Register (TXFF=bit 5, RXFE=bit 4, BUSY=bit 3)
    _reserved2: [u32; 1],
    pub ilpr: u32,     // 0x020: IrDA Low-Power Counter
    pub ibrd: u32,     // 0x024: Integer Baud Rate Divisor
    pub fbrd: u32,     // 0x028: Fractional Baud Rate Divisor
    pub lcr_h: u32,    // 0x02C: Line Control Register (WLEN 8-bit, FEN=FIFO enable)
    pub cr: u32,       // 0x030: Control Register (UARTEN=bit 0, TXE=bit 8, RXE=bit 9)
    pub ifls: u32,     // 0x034: Interrupt FIFO Level Select
    pub imsc: u32,     // 0x038: Interrupt Mask Set/Clear
}

pub struct Pl011Uart {
    pub base_addr: usize,
}

impl Pl011Uart {
    pub const QEMU_VIRT_BASE: usize = 0x0900_0000;
    pub const RPI4_BASE: usize = 0xFE20_1000;

    pub const fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    /// Inicializa el controlador UART PL011 a 115200 baudios, 8N1 con FIFOs activas
    pub fn init(&self) {
        unsafe {
            let regs = self.base_addr as *mut Pl011Registers;
            // Deshabilitar UART durante la configuración
            core::ptr::write_volatile(&mut (*regs).cr, 0);
            // Configurar divisor para 115200 baudios con reloj base de 24MHz (IBRD=13, FBRD=1)
            core::ptr::write_volatile(&mut (*regs).ibrd, 13);
            core::ptr::write_volatile(&mut (*regs).fbrd, 1);
            // 8 bits de palabra (WLEN=3 en bits 5-6) y FIFOs habilitadas (FEN=bit 4) -> 0x70
            core::ptr::write_volatile(&mut (*regs).lcr_h, 0x70);
            // Máscara de interrupciones
            core::ptr::write_volatile(&mut (*regs).imsc, 0);
            // Habilitar UART (bit 0), TX (bit 8) y RX (bit 9) -> 0x301
            core::ptr::write_volatile(&mut (*regs).cr, 0x0301);
        }
    }

    /// Comprueba si el búfer transmisor FIFO está lleno (bit 5 de FR)
    #[inline]
    pub fn is_tx_full(&self) -> bool {
        let flag_reg = (self.base_addr + 0x18) as *const u32;
        unsafe { (core::ptr::read_volatile(flag_reg) & (1 << 5)) != 0 }
    }

    /// Comprueba si el búfer receptor FIFO está vacío (bit 4 de FR)
    #[inline]
    pub fn is_rx_empty(&self) -> bool {
        let flag_reg = (self.base_addr + 0x18) as *const u32;
        unsafe { (core::ptr::read_volatile(flag_reg) & (1 << 4)) != 0 }
    }

    /// Envía un byte por la consola serie esperando a que haya espacio disponible
    pub fn write_byte(&self, byte: u8) {
        while self.is_tx_full() {
            core::hint::spin_loop();
        }
        let data_reg = self.base_addr as *mut u32;
        unsafe {
            core::ptr::write_volatile(data_reg, byte as u32);
        }
    }

    /// Lee un byte de la consola serie si hay datos recibidos
    pub fn read_byte(&self) -> Option<u8> {
        if self.is_rx_empty() {
            None
        } else {
            let data_reg = self.base_addr as *const u32;
            unsafe { Some((core::ptr::read_volatile(data_reg) & 0xFF) as u8) }
        }
    }

    /// Escribe una cadena UTF-8 en la consola UART
    pub fn write_str(&self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
    }
}
