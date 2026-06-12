use core::fmt::{self, Write};
use crate::syscall;

pub struct PortixWriter;

impl Write for PortixWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let ret = syscall::sys_write(1, bytes.as_ptr(), bytes.len());
        if ret < 0 { Err(fmt::Error) } else { Ok(()) }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut w = PortixWriter;
    w.write_fmt(args).unwrap();
}
