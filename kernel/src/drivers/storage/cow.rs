// kernel/src/drivers/storage/cow.rs — Sistema de Archivos Copy-on-Write (CoW) con B-Trees y Snapshots
// Arquitectura transaccional fuera de lugar (out-of-place writes), subvolúmenes y sumas de integridad
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::drivers::serial;

/// Firma mágica canónica del superbloque CoW ("COWS" en ASCII)
pub const COW_MAGIC: u32 = 0x5357_4F43; // Little-endian para "COWS"

/// Clave de indexación en el árbol B-Tree del sistema CoW
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct CowKey {
    pub object_id: u64, // Identificador de inodo o subvolumen
    pub item_type: u8,  // Tipo de item (1 = Inode, 2 = Extent, 3 = Subvol, 4 = DirItem)
    pub offset: u64,    // Desplazamiento lógico dentro del objeto
}

/// Cabecera de un nodo de árbol B-Tree
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CowBTreeNodeHeader {
    pub csum: [u8; 32],       // Suma de verificación SHA-256 del contenido del nodo
    pub generation: u64,      // Número de transacción en la que fue escrito el nodo
    pub block_address: u64,   // Dirección física del bloque donde reside el nodo
    pub level: u8,            // 0 = Hoja (Leaf), 1+ = Nivel interno (Internal)
    pub item_count: u16,      // Cantidad de claves/items almacenados
}

/// Superbloque principal del sistema de archivos Copy-on-Write
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CowSuperblock {
    pub csum: [u8; 32],       // Suma de integridad SHA-256 del superbloque
    pub magic: u32,           // Firma COW_MAGIC
    pub generation: u64,      // Número de generación global (commit atómico)
    pub root_tree_block: u64, // Dirección del bloque raíz del B-Tree maestro
    pub block_size: u32,      // Tamaño de bloque del sistema de archivos (4096 bytes)
    pub total_blocks: u64,    // Capacidad total en bloques
    pub free_blocks: u64,     // Bloques libres disponibles
    pub uuid: [u8; 16],       // Identificador único universal del pool
}

impl CowSuperblock {
    pub const fn new() -> Self {
        Self {
            csum: [0u8; 32],
            magic: COW_MAGIC,
            generation: 1,
            root_tree_block: 256,
            block_size: 4096,
            total_blocks: 65536,
            free_blocks: 65000,
            uuid: [0x50, 0x4f, 0x52, 0x54, 0x49, 0x58, 0x2d, 0x43, 0x4f, 0x57, 0x2d, 0x46, 0x53, 0x2d, 0x30, 0x31],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == COW_MAGIC && self.block_size == 4096
    }
}

/// Descriptor de un subvolumen o snapshot en el sistema de archivos CoW
#[derive(Debug, Clone)]
pub struct CowSubvolume {
    pub id: u64,
    pub name: [u8; 32],
    pub root_block: u64,
    pub is_snapshot: bool,
    pub parent_snapshot_id: u64,
}

/// Gestor de almacenamiento CoW en memoria del kernel
pub struct CowStorageEngine {
    pub superblock: CowSuperblock,
    pub subvolumes: Vec<CowSubvolume>,
    pub dirty_blocks: usize,
}

impl CowStorageEngine {
    pub const fn new() -> Self {
        Self {
            superblock: CowSuperblock::new(),
            subvolumes: Vec::new(),
            dirty_blocks: 0,
        }
    }

    /// Inicializa las estructuras base y el subvolumen raíz (@rootfs)
    pub fn init(&mut self) {
        let mut rootfs_name = [0u8; 32];
        let bytes = b"@rootfs";
        let mut i = 0;
        while i < bytes.len() {
            rootfs_name[i] = bytes[i];
            i += 1;
        }

        self.subvolumes.push(CowSubvolume {
            id: 256,
            name: rootfs_name,
            root_block: 256,
            is_snapshot: false,
            parent_snapshot_id: 0,
        });

        serial::write_str("[COW-FS] Subsistema Copy-on-Write B-Tree inicializado: Gen=");
        serial::write_hex(self.superblock.generation as usize);
        serial::write_str(" (Superblock magic 0x53574F43)\n");
    }

    /// Creación instantánea O(1) de un snapshot de solo lectura
    pub fn create_snapshot(&mut self, subvol_id: u64, snap_name: &[u8]) -> Option<u64> {
        let parent = self.subvolumes.iter().find(|s| s.id == subvol_id)?;
        let parent_root = parent.root_block;

        let new_id = 1000 + self.subvolumes.len() as u64;
        let mut name_buf = [0u8; 32];
        let len = snap_name.len().min(32);
        name_buf[..len].copy_from_slice(&snap_name[..len]);

        self.subvolumes.push(CowSubvolume {
            id: new_id,
            name: name_buf,
            root_block: parent_root, // Comparte la misma raíz en modo CoW
            is_snapshot: true,
            parent_snapshot_id: subvol_id,
        });

        self.superblock.generation += 1;
        Some(new_id)
    }

    /// Reversión atómica (Rollback) de un subvolumen al estado de un snapshot
    pub fn rollback_snapshot(&mut self, target_subvol_id: u64, snapshot_id: u64) -> bool {
        let snap_root = match self.subvolumes.iter().find(|s| s.id == snapshot_id && s.is_snapshot) {
            Some(s) => s.root_block,
            None => return false,
        };

        if let Some(target) = self.subvolumes.iter_mut().find(|s| s.id == target_subvol_id) {
            target.root_block = snap_root;
            self.superblock.generation += 1;
            return true;
        }
        false
    }

    /// Elimina un snapshot del catálogo
    pub fn delete_snapshot(&mut self, snapshot_id: u64) -> bool {
        if let Some(pos) = self.subvolumes.iter().position(|s| s.id == snapshot_id && s.is_snapshot) {
            self.subvolumes.remove(pos);
            self.superblock.generation += 1;
            return true;
        }
        false
    }

    /// Retorna la lista de subvolúmenes y snapshots registrados
    pub fn get_subvolume_count(&self) -> usize {
        self.subvolumes.len()
    }
}

pub static mut COW_ENGINE: CowStorageEngine = CowStorageEngine::new();

/// Inicializador de subsistema de archivos CoW
pub fn cow_init() {
    unsafe {
        COW_ENGINE.init();
    }
}
