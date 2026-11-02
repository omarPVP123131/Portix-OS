// kernel/src/fs/ramfs_adaptador.rs — [CICLO 51 / lote 1] ramfs bajo el contrato
// `SistemaDeFicheros`. No cambia el comportamiento de ramfs: traduce su API
// (bool / Result<_, ()> / errno negativo) a errno POSIX positivo y exacto,
// distinguiendo ENOENT, EEXIST, ENOTDIR, EISDIR, ENOTEMPTY y ENOSPC.

use alloc::string::String;
use alloc::vec::Vec;
use super::{errno, EntradaDir, InformeVerificacion, Metadatos, Resultado, SistemaDeFicheros};
use crate::drivers::storage::ramfs::RamFs;

fn limpiar(ruta: &str) -> &str {
    let c = ruta.trim_end_matches('/');
    if c.is_empty() { "/" } else { c }
}

fn padre(ruta: &str) -> &str {
    let c = limpiar(ruta);
    match c.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &c[..i],
    }
}

impl RamFs {
    fn clasificar(&self, ruta: &str) -> Option<(bool, bool)> {
        let idx = self.find(ruta)?;
        let f = &self.files[idx];
        Some((f.is_dir, f.is_symlink))
    }

    /// ENOENT si falta el padre, ENOTDIR si el padre no es directorio.
    fn comprobar_padre(&self, ruta: &str) -> Resultado<()> {
        let p = padre(ruta);
        // La raíz de ramfs y los puntos de montaje ("/tmp") existen aunque no
        // estén registrados como nodo propio.
        if p == "/" || p == self.montado_en() { return Ok(()); }
        match self.clasificar(p) {
            None => Err(errno::ENOENT),
            Some((true, _)) => Ok(()),
            Some((false, _)) => Err(errno::ENOTDIR),
        }
    }
}

impl SistemaDeFicheros for RamFs {
    fn tipo(&self) -> &'static str { "ramfs" }
    fn montado_en(&self) -> &str { self.prefijo_montaje() }
    fn fijar_montaje(&mut self, prefijo: &str) { self.fijar_prefijo_montaje(prefijo); }

    fn metadatos(&self, ruta: &str) -> Resultado<Metadatos> {
        let c = limpiar(ruta);
        if c == "/" || c == self.montado_en() {
            return Ok(Metadatos { modo: 0o040755, tam: 4096, ino: 1, nlink: 2, bloques: 8, ..Default::default() });
        }
        let idx = self.find(c).ok_or(errno::ENOENT)?;
        let f = &self.files[idx];
        let tam = if f.is_dir { 4096 } else { f.data.len() as u64 };
        Ok(Metadatos {
            modo: f.mode,
            uid: f.uid,
            gid: f.gid,
            tam,
            ino: (idx as u64) + 2,
            nlink: if f.is_dir { 2 } else { 1 },
            mtime: f.mtime as u64,
            bloques: (tam + 511) / 512,
        })
    }

    fn leer(&mut self, ruta: &str, desplazamiento: u64, buf: &mut [u8]) -> Resultado<usize> {
        match self.clasificar(ruta) {
            None => Err(errno::ENOENT),
            Some((true, _)) => Err(errno::EISDIR),
            Some((false, _)) => self.read(ruta, buf, desplazamiento).map_err(|_| errno::EIO),
        }
    }

    fn escribir(&mut self, ruta: &str, desplazamiento: u64, datos: &[u8]) -> Resultado<usize> {
        match self.clasificar(ruta) {
            None => Err(errno::ENOENT),
            Some((true, _)) => Err(errno::EISDIR),
            Some((false, _)) => self.write(ruta, datos, desplazamiento).map_err(|_| errno::EFBIG),
        }
    }

    fn crear(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        if self.find(limpiar(ruta)).is_some() { return Err(errno::EEXIST); }
        self.comprobar_padre(ruta)?;
        if !self.create(ruta) { return Err(errno::ENOSPC); }
        if modo & 0o7777 != 0 { let _ = self.chmod(ruta, modo & 0o7777); }
        Ok(())
    }

    fn truncar(&mut self, ruta: &str, longitud: u64) -> Resultado<()> {
        match self.clasificar(ruta) {
            None => Err(errno::ENOENT),
            Some((true, _)) => Err(errno::EISDIR),
            Some((false, _)) => if self.truncate_to(ruta, longitud as usize) { Ok(()) } else { Err(errno::EIO) },
        }
    }

    fn eliminar(&mut self, ruta: &str) -> Resultado<()> {
        match self.clasificar(ruta) {
            None => Err(errno::ENOENT),
            Some((true, _)) => Err(errno::EISDIR),
            Some((false, _)) => if self.unlink(ruta) { Ok(()) } else { Err(errno::EIO) },
        }
    }

    fn crear_directorio(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        let c = limpiar(ruta);
        if c == "/" || c == self.montado_en() || self.find(c).is_some() { return Err(errno::EEXIST); }
        self.comprobar_padre(ruta)?;
        if !self.mkdir(ruta) { return Err(errno::ENOSPC); }
        if modo != 0 { let _ = self.chmod(c, modo); }
        Ok(())
    }

    fn eliminar_directorio(&mut self, ruta: &str) -> Resultado<()> {
        let c = limpiar(ruta);
        if c == "/" || c == self.montado_en() { return Err(errno::EPERM); }
        match self.clasificar(c) {
            None => Err(errno::ENOENT),
            Some((false, _)) => Err(errno::ENOTDIR),
            Some((true, _)) => if self.rmdir(c) { Ok(()) } else { Err(errno::ENOTEMPTY) },
        }
    }

    fn listar(&self, ruta: &str) -> Resultado<Vec<EntradaDir>> {
        let c = limpiar(ruta);
        if c != "/" && c != self.montado_en() {
            match self.clasificar(c) {
                None => return Err(errno::ENOENT),
                Some((false, _)) => return Err(errno::ENOTDIR),
                _ => {}
            }
        }
        let mut salida = Vec::new();
        let pref = if c == "/" { String::from("/") } else { alloc::format!("{}/", c) };
        for (i, f) in self.files.iter().enumerate() {
            let nombre = match core::str::from_utf8(&f.name[..f.name_len]) { Ok(s) => s.trim_end_matches('/'), Err(_) => continue };
            if !nombre.starts_with(pref.as_str()) || nombre.len() == pref.len() { continue; }
            let resto = &nombre[pref.len()..];
            if resto.is_empty() { continue; }
            if let Some(corte) = resto.find('/') {
                // Subdirectorio implícito (nodo hijo sin nodo propio): se lista una vez.
                let hijo = &resto[..corte];
                if salida.iter().any(|e: &EntradaDir| e.nombre == hijo) { continue; }
                salida.push(EntradaDir { nombre: String::from(hijo), modo: 0o040755, tam: 4096, ino: 0 });
                continue;
            }
            if salida.iter().any(|e: &EntradaDir| e.nombre == resto) { continue; }
            let tam = if f.is_dir { 4096 } else { f.data.len() as u64 };
            salida.push(EntradaDir { nombre: String::from(resto), modo: f.mode, tam, ino: (i as u64) + 2 });
        }
        Ok(salida)
    }

    fn renombrar(&mut self, origen: &str, destino: &str) -> Resultado<()> {
        self.rename(origen, destino).map_err(|e| if e < 0 { -e } else { e })
    }

    fn enlace_simbolico(&mut self, destino: &str, ruta: &str) -> Resultado<()> {
        if self.find(limpiar(ruta)).is_some() { return Err(errno::EEXIST); }
        self.comprobar_padre(ruta)?;
        if self.create_symlink(destino, ruta) { Ok(()) } else { Err(errno::ENOSPC) }
    }

    fn leer_enlace(&self, ruta: &str) -> Resultado<String> {
        match self.clasificar(ruta) {
            None => Err(errno::ENOENT),
            Some((_, false)) => Err(errno::EINVAL),
            Some((_, true)) => {
                let d = self.readlink(ruta).ok_or(errno::EINVAL)?;
                Ok(String::from(core::str::from_utf8(d).map_err(|_| errno::EIO)?))
            }
        }
    }

    fn cambiar_modo(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        self.chmod(ruta, modo).map_err(|e| if e < 0 { -e } else { e })
    }

    fn cambiar_propietario(&mut self, ruta: &str, uid: u32, gid: u32) -> Resultado<()> {
        self.chown(ruta, uid, gid).map_err(|e| if e < 0 { -e } else { e })
    }

    fn enlazar(&mut self, existente: &str, nuevo: &str) -> Resultado<()> {
        self.link(existente, nuevo).map_err(|e| if e < 0 { -e } else { e })
    }

    fn sincronizar(&mut self) -> Resultado<()> { Ok(()) }

    /// Invariantes de ramfs: nombres únicos, ningún fichero con padre que sea
    /// fichero regular, enlaces con destino no vacío, raíz presente.
    fn verificar(&mut self) -> Resultado<InformeVerificacion> {
        let mut inf = InformeVerificacion::default();
        let mut bytes = 0u64;
        for (i, f) in self.files.iter().enumerate() {
            inf.inodos_revisados += 1;
            bytes += f.data.len() as u64;
            let nombre = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("");
            if nombre.is_empty() { inf.incoherencias.push(alloc::format!("nodo {} sin nombre", i)); continue; }
            for (j, g) in self.files.iter().enumerate() {
                if j > i && g.name_len == f.name_len && g.name[..g.name_len] == f.name[..f.name_len] {
                    inf.incoherencias.push(alloc::format!("nombre duplicado '{}' (nodos {} y {})", nombre, i, j));
                }
            }
            if f.is_symlink && f.data.is_empty() {
                inf.incoherencias.push(alloc::format!("enlace '{}' con destino vacío", nombre));
            }
            let p = padre(nombre);
            if p != "/" && p != self.montado_en() {
                if let Some(pi) = self.find(p) {
                    if !self.files[pi].is_dir {
                        inf.incoherencias.push(alloc::format!("'{}' cuelga de '{}', que no es directorio", nombre, p));
                    }
                }
            }
        }
        if self.files.is_empty() || !self.files[0].is_dir {
            inf.incoherencias.push(String::from("raíz ausente o no es directorio"));
        }
        inf.bloques_revisados = (bytes + 4095) / 4096;
        Ok(inf)
    }

    fn espacio(&self) -> (u64, u64, u32) {
        let usados: u64 = self.files.iter().map(|f| ((f.data.len() as u64) + 4095) / 4096).sum();
        let total: u64 = 16 * 1024; // 64 MiB nominales en bloques de 4 KiB
        (total.saturating_sub(usados), total, 4096)
    }
}
