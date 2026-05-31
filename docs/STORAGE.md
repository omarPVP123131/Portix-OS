# Portix OS — Subsistema de Almacenamiento

## Arquitectura

```
Shell / Comandos
    │
    └── drivers::storage::vfs (trait Virtual File System)
          ├── open, read, write, list, mkdir, remove
          │
          └── drivers::storage::fat32 (implementación FAT32)
                ├── mount, root_cluster
                ├── read_file, write_file
                ├── parseo de entradas de directorio (SFN + LFN)
                └── recorrido de cadena de clústeres vía FAT
                      │
                      └── drivers::storage::ata (driver ATA PIO)
                            ├── AtaBus::scan()     — detectar unidades
                            ├── AtaDrive::read_sectors()
                            ├── AtaDrive::write_sectors()
                            ├── LBA48
                            └── caché de sectores (reduce reinicios de bus)
```

> **Nota**: en Portix, el trait VFS y la implementación FAT32 viven ambos
> en `drivers/storage/` (`vfs.rs` y `fat32.rs`). No existe un directorio
> `fs/` separado en la versión actual.

---

## Driver ATA PIO (`drivers/storage/ata.rs`)

### Puertos E/S (primario: 0x1F0–0x1F7 + 0x3F6)

| Puerto | Registro        | Dirección |
|--------|-----------------|-----------|
| 0x1F0  | Datos           | R/W       |
| 0x1F1  | Características | R/W       |
| 0x1F2  | Cuenta de sectores | R/W   |
| 0x1F3  | LBA bajo        | R/W       |
| 0x1F4  | LBA medio       | R/W       |
| 0x1F5  | LBA alto        | R/W       |
| 0x1F6  | Unidad/cabezal  | R/W       |
| 0x1F7  | Comando/Estado  | R/W       |
| 0x3F6  | Control         | W         |

### Protocolo de lectura ATA PIO

```
1. Esperar BSY == 0
2. Escribir cuenta de sectores
3. Escribir LBA bajo/medio/alto (48 bits: dos bancos)
4. Escribir DRV + bit LBA en 0x1F6
5. Enviar comando READ (0x24 para LBA48)
6. Sondear DRQ (status & 0x08)
7. Leer 256 palabras del puerto de datos con INSW
8. Repetir para los sectores restantes
```

### Detección de unidades

`AtaBus::scan()` itera primario/secundario y maestro/esclavo. Para cada
combinación envía el comando IDENTIFY (0xEC) y lee 256 palabras. Parsea:
- Modelo (palabras 27–46)
- Número de serie (palabras 10–19)
- Capacidad LBA48 (palabras 100–103)
- Detección ATAPI vía firma en LBA mid/hi

Los resultados se cachean con `store_primary_drive_info()` para que los
comandos del terminal puedan consultar la información sin re-escanear el bus.

### Tipos principales

```rust
pub enum DriveId { Primary0, Primary1, Secondary0, Secondary1 }

pub struct DriveInfo {
    model:   [u8; 40],
    serial:  [u8; 20],
    size_mb: u64,
    lba48:   bool,
    // ...
}

pub struct AtaDrive { base, ctrl, id, info, ... }

impl AtaDrive {
    pub fn read_sectors(&self, lba: u64, count: usize, buf: &mut [u8]) -> Result<()>;
    pub fn write_sectors(&self, lba: u64, count: usize, buf: &[u8]) -> Result<()>;
    pub fn from_info(info: DriveInfo) -> Self;
}
```

---

## Sistema de archivos FAT32 (`drivers/storage/fat32.rs`)

### Disposición en disco

```
LBA 0          VBR + BPB (Volume Boot Record + BIOS Parameter Block)
LBA 1..N       Sectores reservados
               FAT #1
               FAT #2 (espejo)
               Directorio raíz (cadena de clústeres)
               Clústeres de datos
```

### Campos del BPB usados

| Campo                | Offset | Notas                       |
|----------------------|--------|-----------------------------|
| `bytes_per_sector`   | 0x0B   | 512 (estándar)              |
| `sectors_per_cluster`| 0x0D   | Variable según formato      |
| `reserved_count`     | 0x0E   | Sectores antes de la FAT   |
| `num_fats`           | 0x10   | Normalmente 2               |
| `sectors_per_fat_32` | 0x24   | Tamaño de cada FAT          |
| `root_cluster`       | 0x2C   | Primer clúster del directorio raíz |

### Recorrido de cadena de clústeres

```rust
let mut cluster = root_cluster;
while cluster < 0x0FFF_FFF8 {
    read_cluster(volume, cluster, &mut buf);
    cluster = fat32_read_fat(volume, cluster);
}
```

### Entrada de directorio (32 bytes)

| Offset | Tamaño | Campo            |
|--------|--------|------------------|
| 0      | 8      | Nombre corto (SFN) |
| 8      | 3      | Extensión        |
| 11     | 1      | Atributos        |
| 13     | 1      | Reservado        |
| 20–21  | 2      | Clúster alto     |
| 26–27  | 2      | Clúster bajo     |
| 28–31  | 4      | Tamaño de archivo |

Las entradas LFN (attr = 0x0F) preceden a su entrada de nombre corto y
almacenan hasta 13 caracteres UTF-16 cada una.

---

## MKFS — Formato en primer arranque (`drivers/storage/mkfs.rs`)

Si no se encuentra ningún sistema de archivos FAT32, `mkfs::auto_format()`
escribe la siguiente estructura en disco:

1. MBR con tabla de particiones
2. VBR (FAT32 BPB)
3. FAT inicializada (clúster 2 = fin de cadena)
4. Directorio raíz vacío
5. Directorios estándar: `/bin`, `/etc`, `/home`, `/tmp`, `/usr`, `/var`
6. `README.TXT` en `/home/user`

Tras el formato, el kernel intenta montar el volumen recién creado.
Si el montaje falla de nuevo, `ExplorerState` se inicializa con clúster 2
como fallback.
