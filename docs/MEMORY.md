# Portix OS — Gestión de Memoria

## Regiones de memoria física

Obtenidas de `PortixBootInfo.memory_map`. Cada entrada ocupa 48 bytes:

```rust
struct PortixMemoryRegion {
    base:             u64,  // inicio físico
    length:           u64,  // longitud en bytes
    kind:             u32,  // tipo (ver tabla)
    owner:            u32,  // firmware, cargador, kernel
    reclaim_policy:   u32,  // cuándo puede reclamarse
    cache_attributes: u32,  // UC, WB
}
```

| Constante              | Valor | Descripción                      |
|------------------------|-------|----------------------------------|
| `MEM_USABLE_MAPPED`    | 1     | Usable, identity-mapped          |
| `MEM_USABLE_UNMAPPED`  | 2     | Usable, aún no mapeada           |
| `MEM_RESERVED`         | 3     | Reservada                        |
| `MEM_ACPI_RECLAIM`     | 4     | Reclamable por ACPI              |
| `MEM_ACPI_NVS`         | 5     | ACPI NVS (no volátil)            |
| `MEM_MMIO`             | 6     | Memoria mapeada de E/S           |
| `MEM_FRAMEBUFFER`      | 7     | Framebuffer de vídeo             |
| `MEM_KERNEL`           | 8     | Imagen del kernel                |
| `MEM_LOADER_DATA`      | 12    | Datos del cargador               |
| `MEM_FIRMWARE_RUNTIME` | 15    | Servicios de firmware en runtime |

---

## Rangos reservados (anotaciones semánticas)

```rust
struct ReservedRange {
    base:             u64,
    length:           u64,
    kind:             u32,
    owner:            u32,
    reclaim_policy:   u32,
    cache_attributes: u32,
}
```

Rangos pre-asignados en arranque:

| Base       | Tamaño    | Propósito                | Política de reclamación |
|------------|-----------|--------------------------|-------------------------|
| `0x200000` | variable  | Imagen del kernel        | Nunca                   |
| `0x600000` | `0x1A00`  | PortixBootInfo           | Nunca                   |
| `0x100000` | `0x2000`  | Código del cargador      | Tras init del kernel    |
| `0x700000` | `0x100000`| Datos del cargador       | Tras init del kernel    |
| `0x5000000`| ~4 MB     | Backbuffer gráfico       | Nunca                   |

---

## Heap: Buddy Allocator

**Archivo**: `kernel/src/mem/allocator.rs`

### Diseño

- Base: `~0x500000`, tamaño de pool fijo en compilación
- Bloque mínimo: 64 bytes (`MIN_ORDER`)
- Órdenes `0..~20` (bloque máximo: ~1 MB)
- Listas libres intrusivas: los bloques libres almacenan `next`/`prev`
  en su propia carga útil (sin overhead de metadatos externos)
- Allocator global vía `#[global_allocator] static ALLOCATOR: BuddyAllocator`
- `AllocStats`: contadores atómicos (`AtomicUsize`) visibles desde la UI
  sin bloqueo — `total_allocs`, `total_frees`, `failed_allocs`, `free_blocks[orden]`

### Operaciones públicas

```rust
pub unsafe fn init(&self)
// GlobalAlloc:
unsafe fn alloc(&self, layout: Layout) -> *mut u8
unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout)
unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8
unsafe fn realloc(&self, old: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8
```

### Estrategia

1. Buscar la lista libre de menor orden con bloques disponibles ≥ orden requerido
2. Dividir bloques más grandes de forma recursiva (buddy split)
3. Al liberar: intentar fusión con el bloque buddy si ambos están libres;
   reinsertar en el orden superior
4. La fusión sube iterativamente hasta `MAX_ORDER` o hasta que el buddy no esté libre

### Debug

Con `cfg(debug_assertions)`, cada `alloc`, `dealloc` y fusión emite una traza
por COM1 con el orden y la dirección.

---

## Limitaciones del espacio de direcciones

- Sin gestión propia de tablas de páginas. El kernel opera en el **identity
  map** dejado por el cargador.
- Todas las direcciones físicas = virtuales.
- Trabajo futuro: walk de tablas de páginas, espacio de usuario aislado.

---

## Traducción del mapa de memoria UEFI → Portix

El cargador UEFI (`boot/efi/src/main.rs`) traduce el mapa de memoria de UEFI
al formato de Portix antes de construir `PortixBootInfo`:

| Tipo UEFI                   | Tipo Portix        |
|-----------------------------|--------------------|
| 1 (Loader code)             | `MEM_USABLE`       |
| 4 (Boot services code)      | `MEM_USABLE`       |
| 7 (Conventional memory)     | `MEM_USABLE`       |
| 10 (ACPI reclaim)           | `MEM_ACPI_RECLAIM` |
| 11 (ACPI NVS)               | `MEM_ACPI_NVS`     |
| 13 / 14 (RT code / data)    | `MEM_RESERVED`     |
| 17 (MMIO)                   | `MEM_FRAMEBUFFER`  |
| Resto                       | `MEM_RESERVED`     |

El stage2 BIOS escribe las entradas E820 directamente en formato Portix.
