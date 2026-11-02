use core::fmt::{self, Write};
use crate::syscall;

pub struct PortixStdout;
pub struct PortixStderr;

impl Write for PortixStdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut bytes = s.as_bytes();
        while !bytes.is_empty() {
            let ret = syscall::sys_write(1, bytes.as_ptr(), bytes.len());
            if ret <= 0 { return Err(fmt::Error); }
            let written = (ret as usize).min(bytes.len());
            bytes = &bytes[written..];
        }
        Ok(())
    }
}

impl Write for PortixStderr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut bytes = s.as_bytes();
        while !bytes.is_empty() {
            let ret = syscall::sys_write(2, bytes.as_ptr(), bytes.len());
            if ret <= 0 { return Err(fmt::Error); }
            let written = (ret as usize).min(bytes.len());
            bytes = &bytes[written..];
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::_print_stdout(core::format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", core::format_args!($($arg)*)));
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => ($crate::io::_print_stderr(core::format_args!($($arg)*)));
}

#[macro_export]
macro_rules! eprintln {
    () => ($crate::eprint!("\n"));
    ($($arg:tt)*) => ($crate::eprint!("{}\n", core::format_args!($($arg)*)));
}

/// [85ª, SMP] `println!` escribía cada trozo del formato con un `write(2)`
/// propio ("cuerpo completo: ", "65536", "/", ...). Con dos CPUs, el proceso de
/// la otra CPU se colaba entre trozos y la línea salía rota por la serie (visto
/// en la suite 88a: curl y httpd a la vez). Ahora la línea se compone en un
/// búfer y se emite con un solo `write` (como el stdio de la libc: una línea,
/// una escritura); si no cabe, se vacía por partes de 1 KiB.
struct Linea<'a, W: Write> { buf: [u8; 1024], n: usize, salida: &'a mut W }

impl<'a, W: Write> Linea<'a, W> {
    fn vaciar(&mut self) -> fmt::Result {
        if self.n > 0 {
            // Los trozos entran por `write_str`, así que `buf[..n]` es UTF-8 válido
            // salvo que un carácter multibyte haya partido el límite: en ese caso
            // `from_utf8` falla y se escribe lo que haya como bytes sueltos.
            let n = self.n;
            match core::str::from_utf8(&self.buf[..n]) {
                Ok(t) => { self.salida.write_str(t)?; self.n = 0; }
                Err(e) => {
                    // Se emite el prefijo válido y la cola del carácter partido
                    // se conserva al principio del búfer para la siguiente vuelta.
                    let v = e.valid_up_to();
                    if v > 0 {
                        // SAFETY: `from_utf8` certifica que `[..v]` es UTF-8 válido.
                        let t = unsafe { core::str::from_utf8_unchecked(&self.buf[..v]) };
                        self.salida.write_str(t)?;
                    }
                    self.buf.copy_within(v..n, 0);
                    self.n = n - v;
                    if v == 0 { self.n = 0; } // búfer lleno de bytes inválidos: se descartan
                }
            }
        }
        Ok(())
    }
}

impl<'a, W: Write> Write for Linea<'a, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut b = s.as_bytes();
        while !b.is_empty() {
            let libre = self.buf.len() - self.n;
            if libre == 0 { self.vaciar()?; continue; }
            let k = libre.min(b.len());
            self.buf[self.n..self.n + k].copy_from_slice(&b[..k]);
            self.n += k;
            b = &b[k..];
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print_stdout(args: fmt::Arguments) {
    let mut w = PortixStdout;
    let mut l = Linea { buf: [0u8; 1024], n: 0, salida: &mut w };
    let _ = l.write_fmt(args);
    let _ = l.vaciar();
}

#[doc(hidden)]
pub fn _print_stderr(args: fmt::Arguments) {
    let mut w = PortixStderr;
    let mut l = Linea { buf: [0u8; 1024], n: 0, salida: &mut w };
    let _ = l.write_fmt(args);
    let _ = l.vaciar();
}
