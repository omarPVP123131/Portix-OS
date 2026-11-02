# PORTIX OS — Archivo de Código C Histórico (Migrado 100% a Rust)

Este directorio contiene los prototipos y utilidades originales escritas en C que fueron utilizadas en las primeras fases del sistema operativo.

A partir de la arquitectura modular profesional Senior de Portix OS:
1. **Espacio de Usuario**: Reescrito al 100% en Rust en el workspace `/userland/`.
   - Runtime nativo `portix-rt` (`/userland/crates/portix-rt`) implementando llamadas al sistema, asignador de memoria dinámica, E/S formateada, primitivas de sincronización y abstracciones de red, VFS y gráficos.
   - 15 utilidades y servicios compilados como ejecutables ELF x86_64 nativos (`/userland/bin/`): `hello`, `uptime`, `echo`, `clear`, `mkdir`, `curl`, `lsmod`, `insmod`, `rmmod`, `cat`, `ls`, `sh`, `init`, `test_posix`, `portix-wm`.
2. **Módulos de Kernel (LKM)**: Reescritos al 100% en Rust bare-metal (`/modules/demo_mod/src/lib.rs`).
3. **Servidor Gráfico y Compositor**: Implementado en Ring 3 en Rust desacoplado del núcleo (`/userland/bin/portix-wm`), capaz de mapear directamente el framebuffer físico con aislamiento de privilegios.

El código en esta carpeta se preserva únicamente con fines históricos y de referencia comparativa.
