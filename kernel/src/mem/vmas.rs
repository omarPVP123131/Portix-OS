//! [T-014b] Mapeos PRIVADOS respaldados por fichero, perezosos.
//!
//! `mmap(fd, MAP_PRIVATE)` ya no copia el fichero entero al instante: deja las
//! páginas como reservas perezosas (`paging::mapear_perezosa`) y apunta aquí el
//! tramo `[inicio, fin)` con su fichero y su desplazamiento. En el primer
//! acceso, `paging::materializar_perezosa` asigna el marco, pregunta a esta
//! tabla y rellena el marco con el trozo de fichero (lo que quede más allá del
//! fin del fichero son ceros, como el .bss sobre la cola de un PT_LOAD).
//! Motivo medido (T-026): la JVM mapea `modules` (134 MB) y sólo toca una
//! fracción; copiarlo entero costaba 21,7 s con IF=0 y 34 000 marcos.
//!
//! Clave: el CR3 (los hilos comparten espacio). `fork` duplica las entradas del
//! padre; `munmap` recorta; `free_address_space` olvida el espacio entero.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Clone)]
pub struct VmaFichero {
    pub cr3: u64,
    pub inicio: usize,
    pub fin: usize,
    pub path: [u8; 256],
    pub path_len: usize,
    /// Desplazamiento del fichero que corresponde a `inicio`.
    pub offset: u64,
    /// MAP_SHARED de sólo lectura (el fichero se abrió O_RDONLY): nunca se
    /// escribe de vuelta; `mprotect(PROT_WRITE)` sobre él da EACCES, como Linux.
    pub compartido: bool,
}

static TABLA: Spinlock<Vec<VmaFichero>> = Spinlock::new(Vec::new());
/// Medidas (/proc/smp): páginas materializadas desde fichero y bytes leídos.
pub static PAGINAS_DESDE_FICHERO: AtomicU64 = AtomicU64::new(0);
pub static BYTES_DESDE_FICHERO: AtomicU64 = AtomicU64::new(0);
pub static VMAS_REGISTRADAS: AtomicU64 = AtomicU64::new(0);

/// Registra `[inicio, fin)` como respaldado por `path` desde `offset`. Recorta
/// cualquier tramo previo del mismo espacio que se solape (MAP_FIXED encima).
pub fn registrar(cr3: u64, inicio: usize, fin: usize, path: &[u8], offset: u64, compartido: bool) {
    if fin <= inicio || path.len() > 256 { return; }
    let mut t = TABLA.lock();
    recortar_bloqueada(&mut t, cr3, inicio, fin);
    let mut p = [0u8; 256];
    p[..path.len()].copy_from_slice(path);
    t.push(VmaFichero { cr3, inicio, fin, path: p, path_len: path.len(), offset, compartido });
    VMAS_REGISTRADAS.fetch_add(1, Ordering::Relaxed);
}

/// Quita `[inicio, fin)` de los tramos del espacio `cr3` (munmap, mapeo anónimo encima).
pub fn recortar(cr3: u64, inicio: usize, fin: usize) {
    if fin <= inicio { return; }
    let mut t = TABLA.lock();
    recortar_bloqueada(&mut t, cr3, inicio, fin);
}

fn recortar_bloqueada(t: &mut Vec<VmaFichero>, cr3: u64, inicio: usize, fin: usize) {
    let mut nuevos: Vec<VmaFichero> = Vec::new();
    t.retain_mut(|v| {
        if v.cr3 != cr3 || v.fin <= inicio || v.inicio >= fin { return true; }
        // Solape. Cola que sobrevive a la derecha:
        if v.fin > fin {
            let mut cola = v.clone();
            cola.offset = v.offset + (fin - v.inicio) as u64;
            cola.inicio = fin;
            nuevos.push(cola);
        }
        // Cabeza que sobrevive a la izquierda:
        if v.inicio < inicio { v.fin = inicio; true } else { false }
    });
    t.extend(nuevos);
}

/// Olvida todos los tramos de un espacio de direcciones (exit/execve).
pub fn olvidar(cr3: u64) {
    let mut t = TABLA.lock();
    t.retain(|v| v.cr3 != cr3);
}

/// `fork`: el hijo hereda los tramos del padre (sus PTE perezosas ya se copiaron).
pub fn duplicar(cr3_padre: u64, cr3_hijo: u64) {
    let mut t = TABLA.lock();
    let copia: Vec<VmaFichero> = t.iter().filter(|v| v.cr3 == cr3_padre).map(|v| { let mut c = v.clone(); c.cr3 = cr3_hijo; c }).collect();
    t.extend(copia);
}

/// Si `vaddr` (alineada) cae en un tramo de `cr3`, rellena `marco` (4 KiB, ya a
/// cero) con el trozo de fichero correspondiente. Devuelve true si había tramo.
/// Se llama desde el #PF bajo el cerrojo grande; la tabla se suelta ANTES de
/// leer del VFS (la lectura puede respirar y tardar).
pub fn rellenar_desde_fichero(cr3: u64, vaddr: usize, marco: &mut [u8]) -> bool {
    let (path, path_len, off) = {
        let t = TABLA.lock();
        match t.iter().find(|v| v.cr3 == cr3 && vaddr >= v.inicio && vaddr < v.fin) {
            Some(v) => (v.path, v.path_len, v.offset + (vaddr - v.inicio) as u64),
            None => return false,
        }
    };
    let path_str = match core::str::from_utf8(&path[..path_len]) { Ok(s) => s, Err(_) => return true };
    // Más allá del fin del fichero: ceros (Linux daría SIGBUS a partir de la
    // página siguiente al fin; aquí la cola queda a cero, como el .bss).
    let mut hecho = 0usize;
    while hecho < marco.len() {
        match crate::drivers::storage::vfs::leer_fs(path_str, off + hecho as u64, &mut marco[hecho..]) {
            Ok(0) | Err(()) => break,
            Ok(n) => hecho += n,
        }
    }
    PAGINAS_DESDE_FICHERO.fetch_add(1, Ordering::Relaxed);
    BYTES_DESDE_FICHERO.fetch_add(hecho as u64, Ordering::Relaxed);
    true
}

/// ¿Hay algún tramo MAP_SHARED de sólo lectura que solape `[lo, hi)`?
pub fn hay_compartido_en(cr3: u64, lo: usize, hi: usize) -> bool {
    TABLA.lock().iter().any(|v| v.cr3 == cr3 && v.compartido && v.fin > lo && v.inicio < hi)
}

/// Nº de tramos vivos (diagnóstico).
pub fn vivas() -> usize { TABLA.lock().len() }
