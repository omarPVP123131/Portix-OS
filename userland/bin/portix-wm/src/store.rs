//! Portix-Store — tienda de aplicaciones REAL del escritorio (Ring 3).
//!
//! [FRENTE B-3] Sustituye al panel decorativo "Flathub / Wi-Fi / OOBE". No
//! tiene catálogo incrustado: lee el índice del repositorio
//! (`/var/pkg/index.txt`, descargado por `pkg update` por HTTPS; si no, la
//! semilla `/var/pkg/repo/index.txt`) y el estado real de `/var/pkg/installed.db`.
//! Cada botón lanza `/bin/pkg` con fork + execve + waitpid; `pkg` hace la
//! descarga TLS 1.3, verifica SHA-256 e instala/desinstala de verdad. La
//! tienda sólo refleja lo que el gestor de paquetes hizo (y su código de salida).

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::{println, Framebuffer};
use portix_rt::syscall::{sys_fork, sys_execve, sys_waitpid, sys_exit};
use crate::font;

pub struct StorePkg {
    pub name: String,
    pub size: usize,
    pub sha_prefix: String,
    pub installed: bool,
    pub version: String,
}

pub struct Accion { pid: i64, verbo: String, nombre: String, idx: Option<usize> }

pub struct Store {
    pub pkgs: Vec<StorePkg>,
    pub log: Vec<String>,
    pub index_source: &'static str,
    pub busy: bool,
    pub acciones: usize,
    en_curso: Option<Accion>,
    spinner: u32,
}

fn read_text(path: &str) -> Option<String> {
    portix_rt::fs::read(path).ok().map(|d| String::from_utf8_lossy(&d).into_owned())
}

impl Store {
    pub fn new() -> Self {
        let mut s = Store { pkgs: Vec::new(), log: Vec::new(), index_source: "-", busy: false, acciones: 0, en_curso: None, spinner: 0 };
        s.reload();
        s
    }

    /// Relee índice e instalados desde el VFS (sin caché propia).
    pub fn reload(&mut self) {
        self.pkgs.clear();
        let (idx, src) = match read_text("/var/pkg/index.txt") {
            Some(t) => (t, "/var/pkg/index.txt (pkg update, HTTPS)"),
            None => match read_text("/var/pkg/repo/index.txt") {
                Some(t) => (t, "/var/pkg/repo/index.txt (semilla de la imagen)"),
                None => (String::new(), "sin indice: ejecute 'pkg update'"),
            },
        };
        self.index_source = src;
        let db = read_text("/var/pkg/installed.db").unwrap_or_default();
        for line in idx.lines() {
            let mut it = line.split_whitespace();
            if let (Some(n), Some(sz), Some(h)) = (it.next(), it.next(), it.next()) {
                let mut installed = false; let mut version = String::new();
                for l in db.lines() {
                    let mut p = l.split(':');
                    if p.next() == Some(n) { installed = true; version = String::from(p.next().unwrap_or("")); }
                }
                self.pkgs.push(StorePkg { name: String::from(n), size: sz.parse().unwrap_or(0), sha_prefix: String::from(&h[..h.len().min(12)]), installed, version });
            }
        }
    }

    fn push_log(&mut self, s: String) {
        println!("[STORE] {}", s);
        self.log.push(s);
        while self.log.len() > 4 { self.log.remove(0); }
    }

    /// Lanza `/bin/pkg <verbo> [nombre]` SIN bloquear el compositor: devuelve el
    /// PID del hijo; `poll()` lo recoge con `waitpid(WNOHANG)` en cada vuelta
    /// del bucle de eventos y, al terminar, relee el estado real.
    fn spawn_pkg(&mut self, verbo: &str, nombre: Option<&str>) -> i64 {
        let ruta = b"/bin/pkg\0";
        let a0 = b"pkg\0";
        let mut v = [0u8; 32];
        let vb = verbo.as_bytes(); let vl = vb.len().min(31); v[..vl].copy_from_slice(&vb[..vl]);
        let mut n = [0u8; 64];
        let nb = nombre.unwrap_or("").as_bytes(); let nl = nb.len().min(63); n[..nl].copy_from_slice(&nb[..nl]);
        let argv: [*const u8; 4] = [a0.as_ptr(), v.as_ptr(), if nombre.is_some() { n.as_ptr() } else { core::ptr::null() }, core::ptr::null()];
        let pid = sys_fork();
        if pid == 0 {
            sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
            sys_exit(127);
        }
        pid
    }

    /// Botón "Actualizar": `pkg update` (índice por HTTPS con SHA-256), asíncrono.
    pub fn update(&mut self) {
        if self.busy { return; }
        self.push_log(String::from("pkg update: descargando indice por HTTPS (TLS 1.3)..."));
        let pid = self.spawn_pkg("update", None);
        if pid <= 0 { self.push_log(String::from("fork fallido")); return; }
        self.busy = true;
        self.en_curso = Some(Accion { pid, verbo: String::from("update"), nombre: String::new(), idx: None });
    }

    /// Botón de fila: instala o desinstala según el estado real (asíncrono).
    pub fn toggle(&mut self, idx: usize) {
        if self.busy || idx >= self.pkgs.len() { return; }
        let name = self.pkgs[idx].name.clone();
        let verbo = if self.pkgs[idx].installed { "remove" } else { "install" };
        self.push_log(alloc::format!("pkg {} {} (fork+execve, HTTPS + SHA-256)...", verbo, name));
        let pid = self.spawn_pkg(verbo, Some(&name));
        if pid <= 0 { self.push_log(String::from("fork fallido")); return; }
        self.busy = true;
        self.en_curso = Some(Accion { pid, verbo: String::from(verbo), nombre: name, idx: Some(idx) });
    }

    /// Llamar en cada vuelta del bucle de eventos. Devuelve `true` si cambió algo
    /// (hay que repintar). No bloquea: `waitpid` con WNOHANG.
    pub fn poll(&mut self) -> bool {
        let Some(acc) = self.en_curso.as_ref() else { return false; };
        let mut status: i32 = 0;
        let r = sys_waitpid(acc.pid as i32, &mut status as *mut i32, 1 /* WNOHANG */);
        if r == 0 {
            self.spinner = self.spinner.wrapping_add(1);
            return self.spinner % 8 == 0;
        }
        let acc = self.en_curso.take().unwrap_or(Accion { pid: 0, verbo: String::new(), nombre: String::new(), idx: None });
        let rc = if r != acc.pid { -1 } else if portix_rt::wifexited(status) { portix_rt::wexitstatus(status) } else { 128 + portix_rt::wtermsig(status) };
        self.acciones += 1;
        self.reload();
        if acc.verbo == "update" {
            self.push_log(alloc::format!("pkg update -> rc={} ({} paquetes en el indice)", rc, self.pkgs.len()));
        } else {
            let ahora = acc.idx.and_then(|i| self.pkgs.get(i)).map(|p| p.installed).unwrap_or(false);
            let estado = if ahora { "INSTALADO" } else { "NO instalado" };
            self.push_log(alloc::format!("pkg {} {} -> rc={} ; estado real: {}", acc.verbo, acc.nombre, rc, estado));
            if rc == 0 { println!("[STORE-OK] {} {} (estado real: {})", acc.verbo, acc.nombre, estado); }
            else { println!("[STORE-FAIL] {} {} rc={}", acc.verbo, acc.nombre, rc); }
        }
        self.busy = false;
        true
    }

    pub const ROW_Y0: usize = 92;
    pub const ROW_H: usize = 22;

    /// Devuelve la acción bajo el punto (x, y) relativo a la ventana: fila o botón de actualizar.
    pub fn hit(&self, rel_x: usize, rel_y: usize, win_w: usize) -> Option<StoreHit> {
        if rel_y >= 62 && rel_y < 84 && rel_x >= win_w - 150 && rel_x < win_w - 18 { return Some(StoreHit::Update); }
        if rel_y >= Self::ROW_Y0 {
            let i = (rel_y - Self::ROW_Y0) / Self::ROW_H;
            if i < self.pkgs.len() && rel_x >= win_w - 120 && rel_x < win_w - 18 { return Some(StoreHit::Row(i)); }
        }
        None
    }

    pub fn draw(&self, fb: &Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        fb.fill_rect(x + 10, y + 36, w - 20, h - 46, 0x1a1b26);
        fb.fill_rect(x + 10, y + 36, w - 20, 26, 0x24283b);
        font::draw_str(fb, x + 18, y + 42, "Portix-Store  -  repositorio HTTPS (TLS 1.3) + SHA-256 via /bin/pkg", 0x7aa2f7);
        // Fuente del índice + botón actualizar
        let src = alloc::format!("Indice: {}", self.index_source);
        font::draw_str(fb, x + 18, y + 68, &src, 0xa9b1d6);
        fb.fill_rect(x + w - 150, y + 62, 132, 22, if self.busy { 0x414868 } else { 0x2563eb });
        fb.draw_rect(x + w - 150, y + 62, 132, 22, 0x60a5fa);
        font::draw_str(fb, x + w - 142, y + 66, "[u] Actualizar", 0xffffff);
        // Filas
        if self.pkgs.is_empty() {
            font::draw_str(fb, x + 18, y + Self::ROW_Y0 + 4, "No hay indice. Pulse Actualizar (pkg update).", 0xe0af68);
        }
        for (i, p) in self.pkgs.iter().enumerate() {
            let row_y = y + Self::ROW_Y0 + i * Self::ROW_H;
            if row_y + Self::ROW_H > y + h - 70 { break; }
            fb.fill_rect(x + 18, row_y, w - 36, Self::ROW_H - 2, if i % 2 == 0 { 0x1f2335 } else { 0x16161e });
            let tecla = alloc::format!("[{}] {}", i + 1, p.name);
            font::draw_str(fb, x + 24, row_y + 4, &tecla, 0xffffff);
            let info = alloc::format!("{:>4} KiB  {}..", (p.size + 1023) / 1024, &p.sha_prefix[..8]);
            font::draw_str(fb, x + 150, row_y + 4, &info, 0x9aa5ce);
            let (btn, col, bg) = if p.installed { ("[Quitar]", 0xf7768e, 0x3b1f2b) } else { ("[Instalar]", 0x9ece6a, 0x1f3b2b) };
            fb.fill_rect(x + w - 120, row_y, 102, Self::ROW_H - 2, bg);
            font::draw_str(fb, x + w - 114, row_y + 4, btn, col);
            if p.installed {
                let v = alloc::format!("v{}", p.version);
                font::draw_str(fb, x + w - 190, row_y + 4, &v, 0x73daca);
            }
        }
        // Progreso de la acción en curso (el hijo /bin/pkg sigue ejecutándose; el WM no se bloquea)
        if let Some(acc) = self.en_curso.as_ref() {
            let giro = ['|', '/', '-', '\\'][(self.spinner / 8 % 4) as usize];
            let t = alloc::format!("{} pkg {} {} en curso (PID {})...", giro, acc.verbo, acc.nombre, acc.pid);
            fb.fill_rect(x + 18, y + h - 84, w - 36, 14, 0x1f2937);
            font::draw_str(fb, x + 24, y + h - 82, &t, 0xfbbf24);
        }
        // Registro de acciones reales
        let log_y = y + h - 66;
        fb.fill_rect(x + 18, log_y - 4, w - 36, 1, 0x414868);
        if self.log.is_empty() {
            font::draw_str(fb, x + 18, log_y, "Sin acciones. Teclas 1-9 instalan/quitan; u actualiza.", 0x565f89);
        }
        for (i, l) in self.log.iter().enumerate() {
            let col = if l.contains("rc=0") { 0x9ece6a } else if l.contains("rc=") { 0xf7768e } else { 0xe0af68 };
            let mut t = l.clone(); if t.len() > 70 { t.truncate(70); }
            font::draw_str(fb, x + 18, log_y + i * 14, &t, col);
        }
    }
}

pub enum StoreHit { Update, Row(usize) }
