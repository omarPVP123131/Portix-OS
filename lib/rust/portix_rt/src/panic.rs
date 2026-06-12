use core::panic::PanicInfo;
use crate::syscall;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let msg = if let Some(s) = info.message().as_str() {
        s
    } else {
        "panic occurred"
    };
    let bytes = msg.as_bytes();
    syscall::sys_write(2, bytes.as_ptr(), bytes.len());
    syscall::sys_write(2, "\n".as_ptr(), 1);
    syscall::sys_exit(1)
}
