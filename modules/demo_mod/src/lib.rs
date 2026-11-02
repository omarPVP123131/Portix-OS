#![no_std]

extern "C" {
    fn kprint(msg: *const u8);
    fn register_cmd(name: *const u8, handler: extern "C" fn(*const u8)) -> i32;
    fn unregister_cmd(name: *const u8) -> i32;
    fn register_syscall(num: u64, handler: extern "C" fn(u64, u64, u64, u64, u64) -> i64) -> i32;
    fn unregister_syscall(num: u64) -> i32;
}

extern "C" fn mod_cmd_handler(_args: *const u8) {
    let msg = b"\n  +------------------------------------------------+\n  |   HOLA DESDE EL MODULO LKM EN RUNTIME!        |\n  |   Este comando fue inyectado dinamicamente     |\n  |   sin reiniciar el kernel de PORTIX. (Rust LKM)|\n  +------------------------------------------------+\n\n\0";
    unsafe { kprint(msg.as_ptr()); }
}

extern "C" fn dynamic_sys_handler(_a1: u64, _a2: u64, _a3: u64, _a4: u64, _a5: u64) -> i64 {
    let msg = b"  [DEMO_LKM] Syscall 1001 invocada dinamicamente en Ring 0! (Rust)\n\0";
    unsafe { kprint(msg.as_ptr()); }
    1337
}

#[no_mangle]
pub extern "C" fn init_module() -> i32 {
    let banner = b"\n  [DEMO_LKM] >>> Modulo 'demo_mod' (Rust LKM) inicializado en Ring 0! <<<\n\0";
    let cmd_name = b"modstatus\0";
    unsafe {
        kprint(banner.as_ptr());
        register_cmd(cmd_name.as_ptr(), mod_cmd_handler);
        register_syscall(1001, dynamic_sys_handler);
    }
    0
}

#[no_mangle]
pub extern "C" fn cleanup_module() {
    let banner = b"\n  [DEMO_LKM] >>> Modulo 'demo_mod' (Rust LKM) descargado con exito! <<<\n\0";
    let cmd_name = b"modstatus\0";
    unsafe {
        kprint(banner.as_ptr());
        unregister_cmd(cmd_name.as_ptr());
        unregister_syscall(1001);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
