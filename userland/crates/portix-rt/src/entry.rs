#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rdi, [rsp]",        // rdi = argc
        "lea rsi, [rsp + 8]",    // rsi = argv
        // [CICLO 47-b] rdx = envp = argv + (argc + 1) * 8 (ABI SysV: tras el NULL de argv)
        "lea rdx, [rsi + rdi*8 + 8]",
        "and rsp, -16",          // 16-byte stack alignment for AMD64 ABI call
        "call {main}",
        "mov rdi, rax",          // exit code returned from main
        "xor eax, eax",          // SYS_EXIT = 0
        "int 0x80",
        "2: hlt",
        "jmp 2b",
        main = sym main,
    );
}

extern "C" {
    fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32;
}
