// kernel/src/fs/fat_adaptador.rs — [CICLO 51 / L3c] la partición FAT32 de
// arranque bajo el contrato `SistemaDeFicheros`, de SOLO LECTURA, montada en
// `/boot`. Contiene el cargador (boot.bin, stage2.bin), el núcleo (kernel.bin)
// y el cargador UEFI; es lo que un Linux llamaría la partición de arranque.
//
// Toda escritura devuelve EROFS: el contenido de /boot lo produce la imagen
// (scripts/build.py) y el instalador, nunca el sistema en marcha.
//
// Implementación: cada operación monta el volumen (`Fat32Volume::mount`, que
// solo lee el BPB) sobre el dispositivo 0 del registro, como hace el resto del
// núcleo. El coste es una lectura de sector por operación; aceptable para una
// partición de arranque que se consulta rara vez.

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::storage::fat32::{DirEntryInfo, Fat32Volume};
use crate::drivers::storage::registry;
use super::{errno, EntradaDir, InformeVerificacion, Metadatos, Resultado, SistemaDeFicheros};

pub struct FatSoloLectura {
    prefijo: String,
    dispositivo: usize,
}

impl FatSoloLectura {
    pub fn nuevo(dispositivo: usize) -> Self {
        FatSoloLectura { prefijo: String::from("/boot"), dispositivo }
    }

    /// Ruta relativa al punto de montaje, sin barras sobrantes ("" = raíz).
    fn relativa<'a>(&self, ruta: &'a str) -> &'a str {
        let r = ruta.trim_end_matches('/');
        let r = if r.starts_with(self.prefijo.as_str()) { &r[self.prefijo.len()..] } else { r };
        r.trim_start_matches('/')
    }

    fn con_volumen<R>(&self, f: impl FnOnce(&mut Fat32Volume) -> Resultado<R>) -> Resultado<R> {
        registry::with_device(self.dispositivo, |d| match d {
            Some(drive) => match Fat32Volume::mount(drive) {
                Ok(mut vol) => f(&mut vol),
                Err(_) => Err(errno::EIO),
            },
            None => Err(errno::EIO),
        })
    }

    /// Entrada FAT de `rel` (None = raíz del volumen).
    fn buscar(vol: &mut Fat32Volume, rel: &str) -> Resultado<Option<DirEntryInfo>> {
        if rel.is_empty() { return Ok(None); }
        let root = vol.root_cluster();
        let (dir, nombre) = crate::drivers::storage::fat32::resolve_path(vol, root, rel).map_err(|_| errno::ENOENT)?;
        if nombre.is_empty() { return Ok(None); }
        match vol.find_entry(dir, nombre) {
            Ok(e) => Ok(Some(e)),
            Err(_) => Err(errno::ENOENT),
        }
    }

    fn meta_de(e: &DirEntryInfo, ino: u64) -> Metadatos {
        let modo = if e.is_dir { 0o040555 } else { 0o100555 };
        let tam = if e.is_dir { 4096 } else { e.size as u64 };
        Metadatos { modo, uid: 0, gid: 0, tam, ino, nlink: 1, mtime: 0, bloques: (tam + 511) / 512 }
    }
}

impl SistemaDeFicheros for FatSoloLectura {
    fn tipo(&self) -> &'static str { "fat32-ro" }
    fn montado_en(&self) -> &str { &self.prefijo }
    fn fijar_montaje(&mut self, prefijo: &str) { self.prefijo = String::from(prefijo.trim_end_matches('/')); }

    fn metadatos(&self, ruta: &str) -> Resultado<Metadatos> {
        let rel = self.relativa(ruta);
        self.con_volumen(|vol| match Self::buscar(vol, rel)? {
            None => Ok(Metadatos { modo: 0o040555, tam: 4096, ino: 1, nlink: 2, bloques: 8, ..Default::default() }),
            Some(e) => Ok(Self::meta_de(&e, e.cluster as u64 + 2)),
        })
    }

    fn leer(&mut self, ruta: &str, desplazamiento: u64, buf: &mut [u8]) -> Resultado<usize> {
        let rel = self.relativa(ruta);
        self.con_volumen(|vol| {
            let e = Self::buscar(vol, rel)?.ok_or(errno::EISDIR)?;
            if e.is_dir { return Err(errno::EISDIR); }
            let tam = e.size as u64;
            if desplazamiento >= tam { return Ok(0); }
            let n = ((tam - desplazamiento) as usize).min(buf.len());
            vol.read_file_at(&e, desplazamiento as usize, &mut buf[..n]).map_err(|_| errno::EIO)
        })
    }

    fn escribir(&mut self, _ruta: &str, _d: u64, _datos: &[u8]) -> Resultado<usize> { Err(errno::EROFS) }
    fn crear(&mut self, _ruta: &str, _modo: u32) -> Resultado<()> { Err(errno::EROFS) }
    fn truncar(&mut self, _ruta: &str, _l: u64) -> Resultado<()> { Err(errno::EROFS) }
    fn eliminar(&mut self, _ruta: &str) -> Resultado<()> { Err(errno::EROFS) }
    fn crear_directorio(&mut self, _ruta: &str, _modo: u32) -> Resultado<()> { Err(errno::EROFS) }
    fn eliminar_directorio(&mut self, _ruta: &str) -> Resultado<()> { Err(errno::EROFS) }
    fn renombrar(&mut self, _o: &str, _d: &str) -> Resultado<()> { Err(errno::EROFS) }
    fn enlace_simbolico(&mut self, _d: &str, _r: &str) -> Resultado<()> { Err(errno::EROFS) }
    fn cambiar_modo(&mut self, _ruta: &str, _modo: u32) -> Resultado<()> { Err(errno::EROFS) }
    fn cambiar_propietario(&mut self, _ruta: &str, _uid: u32, _gid: u32) -> Resultado<()> { Err(errno::EROFS) }
    fn enlazar(&mut self, _existente: &str, _nuevo: &str) -> Resultado<()> { Err(errno::EPERM) }
    fn leer_enlace(&self, _ruta: &str) -> Resultado<String> { Err(errno::EINVAL) }
    fn sincronizar(&mut self) -> Resultado<()> { Ok(()) }

    fn listar(&self, ruta: &str) -> Resultado<Vec<EntradaDir>> {
        let rel = self.relativa(ruta);
        self.con_volumen(|vol| {
            let cluster = match Self::buscar(vol, rel)? {
                None => vol.root_cluster(),
                Some(e) if e.is_dir => e.cluster,
                Some(_) => return Err(errno::ENOTDIR),
            };
            let mut salida = Vec::new();
            salida.push(EntradaDir { nombre: String::from("."), modo: 0o040555, tam: 4096, ino: cluster as u64 + 2 });
            salida.push(EntradaDir { nombre: String::from(".."), modo: 0o040555, tam: 4096, ino: 1 });
            vol.list_dir(cluster, |e| {
                let n = e.name_str();
                if n == "." || n == ".." { return; }
                let m = Self::meta_de(e, e.cluster as u64 + 2);
                salida.push(EntradaDir { nombre: String::from(n), modo: m.modo, tam: m.tam, ino: m.ino });
            }).map_err(|_| errno::EIO)?;
            Ok(salida)
        })
    }

    /// Invariantes mínimas de la partición de arranque: BPB montable, raíz
    /// listable y presencia del núcleo (`kernel.bin`).
    fn verificar(&mut self) -> Resultado<InformeVerificacion> {
        let mut inf = InformeVerificacion::default();
        let r = self.con_volumen(|vol| {
            let root = vol.root_cluster();
            let mut n = 0u64;
            vol.list_dir(root, |_| n += 1).map_err(|_| errno::EIO)?;
            let nucleo = vol.find_entry(root, "kernel.bin").is_ok();
            Ok((n, nucleo))
        });
        match r {
            Ok((n, nucleo)) => {
                inf.inodos_revisados = n;
                if !nucleo { inf.incoherencias.push(String::from("/boot sin kernel.bin")); }
            }
            Err(_) => inf.incoherencias.push(String::from("FAT de arranque no montable")),
        }
        Ok(inf)
    }

    fn espacio(&self) -> (u64, u64, u32) { (0, 0, 512) }
}
