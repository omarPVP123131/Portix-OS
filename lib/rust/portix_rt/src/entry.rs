use crate::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let argc: i32;
    let argv: *const *const u8;
    let envp: *const *const u8;
    unsafe {
        core::arch::asm!(
            "mov {}, rsi",
            "mov {}, rdi",
            "mov {}, rdx",
            out(reg) argc,
            out(reg) argv,
            out(reg) envp,
            options(nostack),
        );
    }
    let exit_code = main(argc, argv, envp);
    syscall::sys_exit(exit_code)
}

extern "Rust" {
    fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32;
}
