const char msg[] = "OK";

void _start() {
    asm volatile(
        "mov $1, %%rax\n"
        "mov $1, %%rdi\n"
        "mov $2, %%rdx\n"
        "lea msg(%%rip), %%rsi\n"
        "int $0x80\n"
        "mov $3, %%rax\n"
        "int $0x80\n"
        :
        :
        : "rax", "rdi", "rsi", "rdx"
    );
}
