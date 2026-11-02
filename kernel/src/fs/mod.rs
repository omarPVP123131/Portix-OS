// kernel/src/fs/mod.rs — [CICLO 51 / lote 1] Capa única de sistemas de ficheros
//
// Hasta este ciclo cada syscall de `syscall/mod.rs` decidía ruta a ruta si un
// camino era ramfs, FAT32, devfs o procfs y hablaba con cada uno con su API
// propia (~70 bifurcaciones). Cualquier sistema de ficheros nuevo (ext2 RW,
// el destino del ciclo 51) habría exigido tocar las 70.
//
// Este módulo define UN contrato, `SistemaDeFicheros`, que implementan los
// sistemas montados por el VFS, y UN tipo de descriptor (`FsFileInfo`, antes
// `FsFileInfo`) que sólo guarda ruta + posición + banderas: el descriptor no
// sabe a qué sistema pertenece, lo resuelve el VFS en cada operación.
//
// Convenciones:
//   • Las rutas que recibe el trait son ABSOLUTAS (como las guarda ramfs);
//     cada sistema conoce su prefijo de montaje (`montado_en`) y lo retira si
//     su formato interno trabaja con rutas relativas (ext2).
//   • Los errores son `errno` POSITIVOS (`Err(libc::ENOENT)`); el syscall los
//     niega al devolverlos. Nada de `-1` genérico: cada operación distingue
//     ENOENT / EEXIST / ENOTDIR / EISDIR / ENOTEMPTY / ENOSPC / EINVAL.
//   • `verificar()` es el verificador de invariantes interno (compromiso 1 del
//     ciclo 51): cada sistema comprueba su propia coherencia y devuelve un
//     informe; la recertificación lo contrasta con `e2fsck` del host.

use alloc::string::String;
use alloc::vec::Vec;

pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EIO: i32 = 5;
    pub const EBADF: i32 = 9;
    pub const EEXIST: i32 = 17;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const EROFS: i32 = 30;
    pub const ENAMETOOLONG: i32 = 36;
    pub const ENOTEMPTY: i32 = 39;
    pub const EMLINK: i32 = 31;
    pub const ENOSYS: i32 = 38;
    pub const ELOOP: i32 = 40;
}

pub type Resultado<T> = Result<T, i32>;

/// Metadatos de una entrada (lo que `stat` necesita).
#[derive(Clone, Copy, Debug, Default)]
pub struct Metadatos {
    pub modo: u32,      // S_IFMT | permisos
    pub uid: u32,
    pub gid: u32,
    pub tam: u64,
    pub ino: u64,
    pub nlink: u32,
    pub mtime: u64,
    pub bloques: u64,   // en unidades de 512 B
}

impl Metadatos {
    pub fn es_dir(&self) -> bool { self.modo & 0o170000 == 0o040000 }
    pub fn es_enlace(&self) -> bool { self.modo & 0o170000 == 0o120000 }
    pub fn es_regular(&self) -> bool { self.modo & 0o170000 == 0o100000 }
}

/// Entrada de directorio tal como la devuelve `listar`.
#[derive(Clone, Debug)]
pub struct EntradaDir {
    pub nombre: String,
    pub modo: u32,
    pub tam: u64,
    pub ino: u64,
}

impl EntradaDir {
    pub fn es_dir(&self) -> bool { self.modo & 0o170000 == 0o040000 }
}

/// Informe del verificador de invariantes de un sistema de ficheros.
#[derive(Clone, Debug, Default)]
pub struct InformeVerificacion {
    pub inodos_revisados: u64,
    pub bloques_revisados: u64,
    pub incoherencias: Vec<String>,
}

impl InformeVerificacion {
    pub fn limpio(&self) -> bool { self.incoherencias.is_empty() }
}

/// Contrato único de un sistema de ficheros montado.
pub trait SistemaDeFicheros: Send {
    /// Nombre del tipo ("ramfs", "ext2", ...).
    fn tipo(&self) -> &'static str;
    /// Prefijo de montaje absoluto ("/tmp"). Se fija al montar.
    fn montado_en(&self) -> &str;
    fn fijar_montaje(&mut self, prefijo: &str);

    fn metadatos(&self, ruta: &str) -> Resultado<Metadatos>;
    /// [T-002] lstat(2): metadatos del propio nodo sin seguir un enlace simbólico final.
    fn metadatos_sin_seguir(&self, ruta: &str) -> Resultado<Metadatos> { self.metadatos(ruta) }
    fn leer(&mut self, ruta: &str, desplazamiento: u64, buf: &mut [u8]) -> Resultado<usize>;
    fn escribir(&mut self, ruta: &str, desplazamiento: u64, datos: &[u8]) -> Resultado<usize>;
    fn crear(&mut self, ruta: &str, modo: u32) -> Resultado<()>;
    fn truncar(&mut self, ruta: &str, longitud: u64) -> Resultado<()>;
    fn eliminar(&mut self, ruta: &str) -> Resultado<()>;
    fn crear_directorio(&mut self, ruta: &str, modo: u32) -> Resultado<()>;
    fn eliminar_directorio(&mut self, ruta: &str) -> Resultado<()>;
    fn listar(&self, ruta: &str) -> Resultado<Vec<EntradaDir>>;
    fn renombrar(&mut self, origen: &str, destino: &str) -> Resultado<()>;
    fn enlace_simbolico(&mut self, destino: &str, ruta: &str) -> Resultado<()>;
    fn leer_enlace(&self, ruta: &str) -> Resultado<String>;
    fn cambiar_modo(&mut self, ruta: &str, modo: u32) -> Resultado<()>;
    /// [T-001] chown(2): uid/gid = u32::MAX significa "no cambiar".
    fn cambiar_propietario(&mut self, ruta: &str, uid: u32, gid: u32) -> Resultado<()>;
    /// [T-001] link(2): enlace duro `nuevo` -> inodo de `existente`.
    /// EPERM si `existente` es un directorio, EEXIST si `nuevo` ya existe.
    fn enlazar(&mut self, existente: &str, nuevo: &str) -> Resultado<()>;
    /// Vuelca a disco lo pendiente y ESPERA a que termine. ramfs: no-op.
    fn sincronizar(&mut self) -> Resultado<()>;
    /// Verificador de invariantes interno.
    fn verificar(&mut self) -> Resultado<InformeVerificacion>;
    /// Bloques libres/totales y tamaño de bloque, para `statvfs`.
    fn espacio(&self) -> (u64, u64, u32);
    /// [T-001] (aciertos, fallos, bytes) de la caché de bloques; `None` si el
    /// sistema no tiene disco detrás (ramfs).
    fn cache_bloques(&self) -> Option<(u64, u64, usize)> { None }
    /// [T-001] Descarta la caché de bloques (segura: es de escritura inmediata).
    /// Lo usa `bench` para medir una lectura realmente fría.
    fn descartar_cache_bloques(&mut self) {}
}

pub mod ramfs_adaptador;
pub mod ext2;
pub mod fat_adaptador;
