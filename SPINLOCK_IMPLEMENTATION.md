# Aplicación de Spinlocks - Guía de Implementación

## Objetivo
Proteger los 15 `static mut` sin sincronización para prevenir race conditions en acceso concurrente (SMP, IRQs).

## Módulo Spinlock Disponible
- **Ubicación**: `kernel/src/arch/spinlock.rs`
- **Re-exportado en**: `kernel/src/arch/mod.rs` como `pub use spinlock::Spinlock`
- **Acceso**: `use crate::arch::Spinlock`

## Patrón General

### Antes (INSEGURO - Race Condition)
```rust
static mut KBD_BUF: [u8; 256] = [0u8; 256];
static mut KBD_HEAD: usize = 0;
static mut KBD_TAIL: usize = 0;

// IRQ thread
pub fn keyboard_irq() {
    unsafe {
        KBD_BUF[KBD_HEAD] = read_key();  // Race!
        KBD_HEAD += 1;
    }
}

// Syscall thread
pub fn sys_read() {
    unsafe {
        let ch = KBD_BUF[KBD_TAIL];      // Race!
        KBD_TAIL += 1;
    }
}
```

### Después (SEGURO - Spinlock Protegido)
```rust
struct KeyboardBuffer {
    buf: [u8; 256],
    head: usize,
    tail: usize,
}

static KBD_BUFFER: Spinlock<KeyboardBuffer> = Spinlock::new(KeyboardBuffer {
    buf: [0u8; 256],
    head: 0,
    tail: 0,
});

// IRQ thread
pub fn keyboard_irq() {
    let mut guard = KBD_BUFFER.lock();
    guard.buf[guard.head] = read_key();  // Safe - lock held
    guard.head = guard.head.wrapping_add(1);
}

// Syscall thread
pub fn sys_read() {
    let mut guard = KBD_BUFFER.lock();
    let ch = guard.buf[guard.tail];  // Safe - lock held
    guard.tail = guard.tail.wrapping_add(1);
}
```

---

## Los 15 static mut a Proteger

### 1. kernel/src/syscall.rs:120-122
**Problema**: Dos threads accediendo KBD_BUF simultáneamente

**Antes**:
```rust
static mut KBD_BUF: [u8; KBD_BUF_SIZE] = [0u8; KBD_BUF_SIZE];
static mut KBD_BUF_HEAD: usize = 0;
static mut KBD_BUF_TAIL: usize = 0;
```

**Después**:
```rust
struct KbdBuffer {
    data: [u8; KBD_BUF_SIZE],
    head: usize,
    tail: usize,
}

static KBD_BUFFER: Spinlock<KbdBuffer> = Spinlock::new(KbdBuffer {
    data: [0u8; KBD_BUF_SIZE],
    head: 0,
    tail: 0,
});

// Actualizar pushes:
// let mut buf = KBD_BUFFER.lock();
// buf.data[buf.head] = byte;
```

---

### 2. kernel/src/syscall.rs:334-339
**Problema**: DUPLICATE keyboard buffer - eliminar completamente

**Antes**:
```rust
static mut KBD: KeyboardState = KeyboardState::new();
static mut CHAR_BUF: [u8; 256] = [0u8; 256];
static mut CHAR_HEAD: usize = 0;
static mut CHAR_TAIL: usize = 0;
```

**Solución**: ELIMINAR - es una copia redundante de KBD_BUFFER. Consolidar en KBD_BUFFER únicamente.

---

### 3. kernel/src/process.rs:105-108
**Problema**: PROCESSES array puede ser modificado por context switch + IRQ

**Antes**:
```rust
static mut PROCESSES: [core::mem::MaybeUninit<Process>; MAX_PROCS] = [...];
static mut NEXT_PID: u64 = 1;
```

**Después**:
```rust
struct ProcessTable {
    procs: [core::mem::MaybeUninit<Process>; MAX_PROCS],
    next_pid: u64,
}

static PROCESSES: Spinlock<ProcessTable> = Spinlock::new(ProcessTable {
    procs: [...],
    next_pid: 1,
});

// Actualizar create_process():
// let mut table = PROCESSES.lock();
// table.procs[idx] = MaybeUninit::new(new_process);
```

---

### 4. kernel/src/drivers/storage/vfs.rs:196-209
**Problema**: MOUNT_TABLE modificado por múltiples syscalls concurrentes

**Antes**:
```rust
pub static mut MOUNT_TABLE: [MountEntry; MAX_MOUNTS] = [...];
pub static mut MOUNT_COUNT: usize = 0;
pub static mut RAMFS: Option<RamFs> = None;
```

**Después**:
```rust
pub struct VfsState {
    mount_table: [MountEntry; MAX_MOUNTS],
    mount_count: usize,
    ramfs: Option<RamFs>,
}

pub static VFS: Spinlock<VfsState> = Spinlock::new(VfsState {
    mount_table: [...],
    mount_count: 0,
    ramfs: None,
});

// Uso:
// let mut vfs = VFS.lock();
// vfs.mount_table[...] = entry;
```

---

### 5. kernel/src/drivers/storage/registry.rs:35
**Problema**: get_device() devuelve &'static mut sin protección

**Antes**:
```rust
static mut REGISTRY: DeviceRegistry = ...;

pub fn get_device(id: usize) -> Option<&'static mut dyn BlockDevice> {
    unsafe { /* acceso sin lock */ }
}
```

**Después**:
```rust
static REGISTRY: Spinlock<DeviceRegistry> = Spinlock::new(...);

pub fn get_device(id: usize) -> Option<SpinlockGuard<'static, dyn BlockDevice>> {
    let registry = REGISTRY.lock();
    // Devolver guard que mantiene lock
}
```

---

### 6. kernel/src/drivers/storage/ata.rs:649
**Problema**: CACHED_DRIVE acceso concurrente desde múltiples threads

**Solución**: Envolver en Spinlock<CachedDrive>

---

### 7. kernel/src/console/terminal/commands/disk.rs:81
**Problema**: VOL_CACHE acceso concurrente

**Solución**: Envolver en Spinlock

---

### 8-9. kernel/src/ipc.rs:40-41
**Problema**: MAILBOXES y IRQ_ROUTES sin sincronización

**Antes**:
```rust
static mut MAILBOXES: [PerProcQueue; MAX_PROCS] = ...;
static mut IRQ_ROUTES: [Option<u64>; MAX_IRQ] = ...;
```

**Después**:
```rust
struct IPCState {
    mailboxes: [PerProcQueue; MAX_PROCS],
    irq_routes: [Option<u64>; MAX_IRQ],
}

static IPC: Spinlock<IPCState> = Spinlock::new(...);
```

---

### 10-13. kernel/src/arch/idt.rs:68-101
**Problema**: DF_STACK, TSS, GDT, IDT - estructuras globales sin sincronización

**NOTA**: Estas son bastante críticas porque se inicializan UNA SOLA VEZ.
**Solución**: Usar lazy_static o inicialización segura, posiblemente sin Spinlock en estos casos (init-once).

---

### 14-15. kernel/src/arch/isr_handlers.rs
**Problema**: exception_cs, crash_frame, SCANCODE_BUF, MOUSE_BUF, etc.

**Solución**: Agrupar en estructura y proteger con Spinlock.

---

## Implementación Paso a Paso

### Paso 1: Importar Spinlock
```rust
use crate::arch::Spinlock;
```

### Paso 2: Crear estructura
```rust
struct MyState {
    field1: Type1,
    field2: Type2,
}
```

### Paso 3: Declarar static
```rust
static MY_STATE: Spinlock<MyState> = Spinlock::new(MyState {
    field1: default_val1,
    field2: default_val2,
});
```

### Paso 4: Actualizar accesos
```rust
// Antes
unsafe {
    MY_FIELD = new_value;
}

// Después
let mut guard = MY_STATE.lock();
guard.field = new_value;
drop(guard);  // Optional - dropped automatically
```

---

## Consideraciones de Performance

1. **Contention**: Si muchos threads compiten por el lock → spinwait costoso
   - **Solución**: Usar locks finos (separar por CPU o dispositivo)

2. **Lock Duration**: Los locks NO deben durar largo tiempo
   - Soltar antes de I/O, allocations, yield
   - Ideal: < 1 microsegundo

3. **Deadlock Prevention**:
   - Siempre adquirir locks en el MISMO ORDEN
   - Nunca reentrar (spinlock no es reentrant)

---

## Validación

Después de aplicar cambios:

1. Compilar sin warnings
   ```bash
   cargo +nightly check --release -Z build-std=core,alloc
   ```

2. Buscar `static mut` restantes
   ```bash
   grep -r "static mut" kernel/src/
   ```

3. Testing bajo load
   ```bash
   # Crear múltiples procesos que compitan por recursos
   python scripts/build.py
   # En QEMU: stress test
   ```

---

## Rollout Recomendado

1. ✅ **Ya completado**: Spinlock implementado
2. ⏳ **Fase 1** (Crítico): KBD_BUF, PROCESSES, MOUNT_TABLE
3. ⏳ **Fase 2** (Importante): IPC, ATA, registry
4. ⏳ **Fase 3** (Bajo): Buffers menores, excepciones

---

## Debugging

Si hay problemas con deadlock:

```rust
// Agregar logging
let mut guard = match MY_STATE.try_lock() {
    Some(g) => {
        serial::log("Lock acquired\n");
        g
    }
    None => {
        serial::log("DEADLOCK: Could not acquire lock!\n");
        panic!("Deadlock");
    }
};
```
