# Portix OS — Roadmap

## Estado actual: v1.0.0

Portix OS es un sistema operativo de 64 bits (x86_64) en desarrollo activo, escrito 100% en Rust (`no_std`).

### Completado y verificado

- Núcleo freestanding con gestión de memoria (buddy allocator, paginación 4 niveles, COW).
- Compatibilidad binaria Linux (452 syscalls traducidas).
- Sistemas de archivos: FAT32, ext2 RW, ramfs, procfs.
- Red: TCP/IP, IPv6, TLS 1.3 real en Ring 3.
- SMP real (2 núcleos).
- Señales POSIX, hilos, futex.
- Entorno gráfico (compositor Wayland en Ring 3).
- Shell POSIX con control de trabajos.
- Toolchain propio (portix-cc, portix-bench, dbg).
- Gestor de paquetes (pkg: .apk Alpine, .deb Debian).
- Drivers bare-metal: NVMe, AHCI/SATA, xHCI/USB, HDA/audio (sobre QEMU).

### En progreso

- Tickets abiertos: ver `docs/TICKETS.md`.
- ext2: indirecto triple, caché de escritura, enlaces duros, fsync por fichero.
- Profiler: histogramas por símbolo del ELF.
- Reducción del fallback del traductor Linux ABI.

### Pendiente

- Drivers bare-metal en hardware físico real.
- Actualización atómica A/B del sistema.
- Imagen reproducible bit a bit.
- Wayland completo en portix-wm (actualmente parcial).

### Documentación

- `docs/TICKETS.md` — tickets abiertos y cerrados
- `docs/PORTIX_STATUS_REAL_VERIFICADO.md` — auditoría técnica honesta
- `AGENTS.md` — bitácora de desarrollo
- `CHANGELOG.md` — historial de cambios