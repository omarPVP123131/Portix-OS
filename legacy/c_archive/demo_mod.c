/* modules/demo_mod.c — PORTIX Loadable Kernel Module (LKM) Demo
 *
 * Demonstrates:
 * 1. Dynamic kernel code execution in Ring 0
 * 2. External kernel symbol binding (kprint, register_cmd, register_syscall)
 * 3. Dynamic shell command injection at runtime ("modstatus")
 * 4. Dynamic syscall registration (syscall 101)
 * 5. Clean resource deallocation and unregistration upon unload
 */

extern void kprint(const char *msg);
extern unsigned long get_ticks(void);
extern unsigned long get_uptime(void);
extern int register_cmd(const char *name, void (*handler)(const char *args));
extern int unregister_cmd(const char *name);
extern int register_syscall(unsigned long num, long (*handler)(unsigned long, unsigned long, unsigned long, unsigned long, unsigned long, unsigned long));
extern int unregister_syscall(unsigned long num);

static void mod_cmd_handler(const char *args) {
    kprint("\n  +------------------------------------------------+\n");
    kprint("  |   HOLA DESDE EL MODULO LKM EN RUNTIME!        |\n");
    kprint("  |   Este comando fue inyectado dinamicamente     |\n");
    kprint("  |   sin reiniciar el kernel de PORTIX.           |\n");
    kprint("  +------------------------------------------------+\n\n");
}

static long dynamic_sys_handler(unsigned long a1, unsigned long a2, unsigned long a3, unsigned long a4, unsigned long a5, unsigned long a6) {
    kprint("  [DEMO_LKM] Syscall 101 invocada dinamicamente en Ring 0!\n");
    return 1337;
}

int init_module(void) {
    kprint("\n  [DEMO_LKM] >>> Modulo 'demo_mod' inicializado en Ring 0! <<<\n");
    register_cmd("modstatus", mod_cmd_handler);
    register_syscall(101, dynamic_sys_handler);
    return 0;
}

void cleanup_module(void) {
    kprint("\n  [DEMO_LKM] >>> Modulo 'demo_mod' descargado con exito! <<<\n");
    unregister_cmd("modstatus");
    unregister_syscall(101);
}
