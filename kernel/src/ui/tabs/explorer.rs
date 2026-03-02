// ui/tabs/explorer.rs — PORTIX Kernel v0.7.4
//
// Explorador de archivos FAT32 con:
//  - Panel izquierdo: árbol de rutas (breadcrumb + historial)
//  - Panel derecho: lista de archivos con iconos, tamaño y fecha
//  - Panel inferior: preview de archivo (texto plano)
//  - Scrollbar elegante
//  - Layout sin solapamiento: vive dentro de content_y..bottom_y
//
// LAYOUT INTERNO:
//   [HDR_H = 24px]  → breadcrumb + atajos
//   [colums_h]      → panel izquierdo (árbol) | panel derecho (lista)
//                      ├── col_hdr (14px)  → "Nombre" / "Tam" / "Tipo"
//                      └── list rows
//   [PREVIEW_H = 80px] → vista previa
//   [STATUS_H = 18px]  → status bar
//
// NOTA: usa AtaDrive a través de Fat32Volume — compatible con ata.rs y fat32.rs.

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::drivers::storage::fat32::{DirEntryInfo, Fat32Volume};
use crate::drivers::storage::vfs::VFS_TREE;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::ui::input::{
    draw_input_overlay, InputBox, InputMode, INPUT_BG, INPUT_BG_DELETE, INPUT_MAX,
};

// ─────────────────────────────────────────────────────────────────────────────
// Paleta
// ─────────────────────────────────────────────────────────────────────────────

pub struct ExpPal;
impl ExpPal {
    pub const BG: Color = Color::new(0x07, 0x0C, 0x14);
    pub const PANEL_L_BG: Color = Color::new(0x05, 0x0A, 0x16);
    pub const PANEL_R_BG: Color = Color::new(0x08, 0x0E, 0x1C);
    pub const HDR_BG: Color = Color::new(0x04, 0x0E, 0x24);
    pub const COL_HDR_BG: Color = Color::new(0x0A, 0x16, 0x28);
    pub const ROW_ODD: Color = Color::new(0x07, 0x0D, 0x1A);
    pub const ROW_EVEN: Color = Color::new(0x09, 0x10, 0x1F);
    pub const ROW_SEL: Color = Color::new(0x14, 0x38, 0x74);
    pub const ROW_SEL_BOR: Color = Color::new(0x28, 0x70, 0xE0);
    pub const ROW_HOV: Color = Color::new(0x0E, 0x20, 0x40);
    pub const BORDER: Color = Color::new(0x14, 0x22, 0x3C);
    pub const SEP_BRIGHT: Color = Color::new(0x22, 0x3A, 0x60);
    pub const DIR_FG: Color = Color::new(0xFF, 0xCC, 0x44);
    pub const DIR_ICON: Color = Color::new(0xFF, 0xAA, 0x00);
    pub const FILE_FG: Color = Color::new(0xA8, 0xCC, 0xFF);
    pub const FILE_ICON: Color = Color::new(0x66, 0x99, 0xDD);
    pub const FILE_RS: Color = Color::new(0xDE, 0x6A, 0x40);
    pub const FILE_C: Color = Color::new(0x44, 0xAA, 0xFF);
    pub const FILE_ASM: Color = Color::new(0xCC, 0xAA, 0x00);
    pub const FILE_IMG: Color = Color::new(0x66, 0xCC, 0x88);
    pub const SIZE_FG: Color = Color::new(0x50, 0x7A, 0xAA);
    pub const TYPE_FG: Color = Color::new(0x44, 0x66, 0x88);
    pub const COL_HDR_FG: Color = Color::new(0x55, 0x77, 0xAA);
    pub const TEXT_DIM: Color = Color::new(0x55, 0x6A, 0x88);
    pub const TREE_FG: Color = Color::new(0x66, 0x88, 0xBB);
    pub const TREE_SEL: Color = Color::new(0xFF, 0xD7, 0x00);
    pub const PREVIEW_BG: Color = Color::new(0x05, 0x09, 0x12);
    pub const PREVIEW_FG: Color = Color::new(0x80, 0xA8, 0xCC);
    pub const PREVIEW_HDR: Color = Color::new(0x00, 0xCC, 0x88);
    pub const SCROLLBAR_BG: Color = Color::new(0x0C, 0x14, 0x24);
    pub const SCROLLBAR_FG: Color = Color::new(0x20, 0x50, 0xA0);
    pub const SCROLLBAR_HOV: Color = Color::new(0x30, 0x70, 0xD0);
    pub const STATUS_BG: Color = Color::new(0x08, 0x78, 0xE0);
    pub const STATUS_ERR: Color = Color::new(0x88, 0x10, 0x10);
    pub const GOLD: Color = Color::new(0xFF, 0xD7, 0x00);
}

// ─────────────────────────────────────────────────────────────────────────────
// Constantes de layout interno
// ─────────────────────────────────────────────────────────────────────────────

const HDR_H: usize = 24; // breadcrumb
const COL_HDR_H: usize = 14; // cabecera de columnas
const PREVIEW_H: usize = 82; // panel de preview
const STATUS_H: usize = 18; // status bar del explorer
const TREE_W: usize = 160; // ancho del panel árbol (izquierda)
const SCROLLBAR_W: usize = 8; // scrollbar derecha
const ROW_H: usize = 14; // altura de cada fila

const MAX_ENTRIES: usize = 256;
const MAX_PATH_DEPTH: usize = 32;
const MAX_PATH_LEN: usize = 512;
const PREVIEW_BYTES: usize = 2048;
const PREVIEW_LINES: usize = 5;

// ─────────────────────────────────────────────────────────────────────────────
// PathNode
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PathNode {
    pub name: [u8; 256],
    pub name_len: usize,
    pub cluster: u32,
}

impl PathNode {
    pub const fn root(cluster: u32) -> Self {
        let mut name = [0u8; 256];
        name[0] = b'/';
        PathNode {
            name,
            name_len: 1,
            cluster,
        }
    }
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tipo de archivo (para icono y color)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum FileKind {
    Dir,
    Rust,
    C,
    Asm,
    Text,
    Image,
    Binary,
    Other,
}

fn file_kind(name: &str, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Dir;
    }
    if name.ends_with(".rs") {
        FileKind::Rust
    } else if name.ends_with(".c") || name.ends_with(".h") {
        FileKind::C
    } else if name.ends_with(".asm") || name.ends_with(".s") {
        FileKind::Asm
    } else if name.ends_with(".txt") || name.ends_with(".md") {
        FileKind::Text
    } else if name.ends_with(".bmp") || name.ends_with(".png") {
        FileKind::Image
    } else if name.ends_with(".bin") || name.ends_with(".elf") {
        FileKind::Binary
    } else {
        FileKind::Other
    }
}

fn kind_icon(k: FileKind) -> (&'static str, Color) {
    match k {
        FileKind::Dir => (" [D] ", ExpPal::DIR_ICON),
        FileKind::Rust => (" [rs]", ExpPal::FILE_RS),
        FileKind::C => (" [ c]", ExpPal::FILE_C),
        FileKind::Asm => (" [as]", ExpPal::FILE_ASM),
        FileKind::Text => (" [tx]", ExpPal::FILE_FG),
        FileKind::Image => (" [im]", ExpPal::FILE_IMG),
        FileKind::Binary => (" [bi]", ExpPal::TYPE_FG),
        FileKind::Other => (" [  ]", ExpPal::TYPE_FG),
    }
}

fn kind_name_color(k: FileKind) -> Color {
    match k {
        FileKind::Dir => ExpPal::DIR_FG,
        FileKind::Rust => Color::new(0xE8, 0xB0, 0x90),
        FileKind::C => Color::new(0x88, 0xCC, 0xFF),
        FileKind::Asm => Color::new(0xDD, 0xCC, 0x88),
        _ => ExpPal::FILE_FG,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ExplorerState
// ─────────────────────────────────────────────────────────────────────────────

pub struct ExplorerState {
    pub path_stack: [PathNode; MAX_PATH_DEPTH],
    pub path_depth: usize,
    pub entries: [Option<DirEntryInfo>; MAX_ENTRIES],
    pub entry_count: usize,
    pub selected: usize,
    pub scroll: usize,
    pub preview: [u8; PREVIEW_BYTES],
    pub preview_len: usize,
    pub preview_name: [u8; 256],
    pub preview_nlen: usize,
    pub status: [u8; 80],
    pub status_len: usize,
    pub status_ok: bool,
    // señal de apertura en IDE
    pub open_request: bool,
    pub open_cluster: u32,
    pub open_name: [u8; 256],
    pub open_name_len: usize,
    pub open_size: u32,
    pub needs_refresh: bool,
    // Entrada de texto inline (nueva carpeta, eliminar, etc.)
    pub input: InputBox,
    // Barra lateral VFS: índice del path seleccionado
    pub vfs_sel: usize,
    pub show_vfs: bool, // toggle con Tab
}

impl ExplorerState {
    pub fn new(root_cluster: u32) -> Self {
        const NONE_ENTRY: Option<DirEntryInfo> = None;
        const ROOT_NODE: PathNode = PathNode::root(0);
        let mut s = ExplorerState {
            path_stack: [ROOT_NODE; MAX_PATH_DEPTH],
            path_depth: 1,
            entries: [NONE_ENTRY; MAX_ENTRIES],
            entry_count: 0,
            selected: 0,
            scroll: 0,
            preview: [0u8; PREVIEW_BYTES],
            preview_len: 0,
            preview_name: [0u8; 256],
            preview_nlen: 0,
            status: [0u8; 80],
            status_len: 0,
            status_ok: true,
            open_request: false,
            open_cluster: 0,
            open_name: [0u8; 256],
            open_name_len: 0,
            open_size: 0,
            needs_refresh: true,
            input: InputBox::new(),
            vfs_sel: 0,
            show_vfs: true,
        };
        s.path_stack[0] = PathNode::root(root_cluster);
        s.show_vfs = true;
        s
    }

    pub fn current_cluster(&self) -> u32 {
        self.path_stack[self.path_depth.saturating_sub(1)].cluster
    }

    pub fn set_status(&mut self, msg: &str, ok: bool) {
        let n = msg.len().min(80);
        self.status[..n].copy_from_slice(&msg.as_bytes()[..n]);
        self.status_len = n;
        self.status_ok = ok;
    }

    pub fn refresh(&mut self, vol: &Fat32Volume) {
        self.entry_count = 0;
        const NONE_ENTRY: Option<DirEntryInfo> = None;
        self.entries = [NONE_ENTRY; MAX_ENTRIES];
        let dir_clus = self.current_cluster();
        let mut count = 0usize;
        let entries_ref = &mut self.entries;
        let _ = vol.list_dir(dir_clus, |e| {
            let name = e.name_str();
            if name == "." || name == ".." {
                return;
            }
            if count < MAX_ENTRIES {
                entries_ref[count] = Some(e.clone());
                count += 1;
            }
        });
        self.entry_count = count;
        sort_entries(&mut self.entries, count);
        if self.selected >= count && count > 0 {
            self.selected = count - 1;
        }
        self.needs_refresh = false;
        self.set_status("Directorio cargado.", true);
    }

    pub fn load_preview(&mut self, vol: &Fat32Volume) {
        if let Some(entry) = self.entries[self.selected].as_ref() {
            if entry.is_dir {
                self.preview_len = 0;
                return;
            }
            let mut n = [0u8; 256];
            n[..entry.name_len].copy_from_slice(&entry.name[..entry.name_len]);
            self.preview_name = n;
            self.preview_nlen = entry.name_len;
            let cloned = entry.clone();
            self.preview_len = vol.read_file(&cloned, &mut self.preview).unwrap_or(0);
        } else {
            self.preview_len = 0;
        }
    }

    pub fn selected_entry(&self) -> Option<&DirEntryInfo> {
        if self.selected < self.entry_count {
            self.entries[self.selected].as_ref()
        } else {
            None
        }
    }

    fn try_enter_dir(&mut self) -> bool {
        let (is_dir, cluster, name_len, name) = if let Some(e) = self.selected_entry() {
            let mut n = [0u8; 256];
            n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.is_dir, e.cluster, e.name_len, n)
        } else {
            return false;
        };
        if is_dir && self.path_depth < MAX_PATH_DEPTH {
            self.path_stack[self.path_depth] = PathNode {
                name,
                name_len,
                cluster,
            };
            self.path_depth += 1;
            self.selected = 0;
            self.scroll = 0;
            self.needs_refresh = true;
            self.preview_len = 0;
            true
        } else {
            false
        }
    }

    fn try_open_file(&mut self) -> bool {
        let (cluster, size, name_len, name) = if let Some(e) = self.selected_entry() {
            if e.is_dir {
                return false;
            }
            let mut n = [0u8; 256];
            n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.cluster, e.size, e.name_len, n)
        } else {
            return false;
        };
        self.open_request = true;
        self.open_cluster = cluster;
        self.open_size = size;
        self.open_name = name;
        self.open_name_len = name_len;
        true
    }

    pub fn go_up(&mut self) {
        if self.path_depth > 1 {
            self.path_depth -= 1;
            self.selected = 0;
            self.scroll = 0;
            self.needs_refresh = true;
            self.preview_len = 0;
        }
    }

    pub fn build_path(&self, out: &mut [u8]) -> usize {
        let mut p = 0usize;
        for i in 0..self.path_depth {
            let node = &self.path_stack[i];
            if i == 0 {
                if p < out.len() {
                    out[p] = b'/';
                    p += 1;
                }
            } else {
                for b in node.name_str().bytes() {
                    if p < out.len() {
                        out[p] = b;
                        p += 1;
                    }
                }
                if p < out.len() {
                    out[p] = b'/';
                    p += 1;
                }
            }
        }
        p
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        // Si el input está activo, lo consume todo
        // is_active() devuelve bool — sin borrow de &self.input.mode
        if self.input.is_active() {
            if let Some(confirmed) = self.input.feed(key) {
                // feed() terminó → ahora sí podemos leer mode sin conflicto
                let mode = self.input.mode;
                self.input.close(); // close() requiere &mut, pero feed() ya liberó su borrow
                if confirmed {
                    match mode {
                        InputMode::NewDir => {
                            self.needs_refresh = true;
                            self.set_status("Nueva carpeta (pendiente FAT32).", true);
                        }
                        InputMode::NewFile => {
                            self.needs_refresh = true;
                            self.set_status("Nuevo archivo (pendiente FAT32).", true);
                        }
                        InputMode::Delete => {
                            self.needs_refresh = true;
                            self.set_status("Eliminado (pendiente FAT32).", true);
                        }
                        _ => {}
                    }
                } else {
                    self.set_status("Cancelado.", true);
                }
            }
            return true;
        }

        match key {
            Key::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                self.clamp_scroll(0);
                true
            }
            Key::Down => {
                if self.selected + 1 < self.entry_count {
                    self.selected += 1;
                }
                self.clamp_scroll(0);
                true
            }
            Key::PageUp => {
                self.selected = self.selected.saturating_sub(12);
                self.clamp_scroll(0);
                true
            }
            Key::PageDown => {
                self.selected = (self.selected + 12).min(self.entry_count.saturating_sub(1));
                self.clamp_scroll(0);
                true
            }
            Key::Enter => {
                if !self.try_enter_dir() {
                    self.try_open_file();
                }
                true
            }
            Key::Backspace => {
                self.go_up();
                true
            }
            Key::F5 => {
                self.needs_refresh = true;
                true
            }
            // N = nueva carpeta
            Key::Char(b'n') | Key::Char(b'N') => {
                self.input.start(InputMode::NewDir, "nueva_carpeta");
                self.set_status("Nombre de nueva carpeta (Enter=OK, Esc=Cancelar):", true);
                true
            }
            // F = nuevo archivo
            Key::Char(b'f') | Key::Char(b'F') => {
                self.input.start(InputMode::NewFile, "nuevo.txt");
                self.set_status("Nombre del archivo (Enter=OK, Esc=Cancelar):", true);
                true
            }
            // D o Delete = eliminar selección
            Key::Char(b'd') | Key::Char(b'D') | Key::Delete => {
                // Copiamos los datos que necesitamos ANTES de cualquier borrow mutable
                let maybe_name: Option<([u8; 256], usize)> = self
                    .selected_entry()
                    .filter(|e| !e.is_dir)
                    .map(|e| (e.name, e.name_len));

                if let Some((name_buf, name_len)) = maybe_name {
                    let n = name_len.min(INPUT_MAX);
                    // Construimos un slice de bytes y lo pasamos como &str
                    let name_str = core::str::from_utf8(&name_buf[..n]).unwrap_or("archivo");
                    self.input.start(InputMode::Delete, name_str);
                    self.set_status("Eliminar (Enter=confirmar, Esc=cancelar):", false);
                } else {
                    self.set_status("Nada seleccionado.", false);
                }
                true
            }
            // Tab = toggle panel VFS
            Key::Tab => {
                self.show_vfs = !self.show_vfs;
                true
            }
            _ => false,
        }
    }

    fn clamp_scroll(&mut self, visible_rows: usize) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if visible_rows > 0 && self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected + 1 - visible_rows;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ordenación: directorios primero, luego alfabético
// ─────────────────────────────────────────────────────────────────────────────

fn sort_entries(entries: &mut [Option<DirEntryInfo>; MAX_ENTRIES], count: usize) {
    for i in 0..count {
        for j in i + 1..count {
            let swap = match (&entries[i], &entries[j]) {
                (Some(a), Some(b)) => {
                    if a.is_dir && !b.is_dir {
                        false
                    } else if !a.is_dir && b.is_dir {
                        true
                    } else {
                        name_gt(a, b)
                    }
                }
                _ => false,
            };
            if swap {
                entries.swap(i, j);
            }
        }
    }
}

fn name_gt(a: &DirEntryInfo, b: &DirEntryInfo) -> bool {
    let la = a.name_len.min(16);
    let lb = b.name_len.min(16);
    for i in 0..la.min(lb) {
        let ca = a.name[i].to_ascii_lowercase();
        let cb = b.name[i].to_ascii_lowercase();
        if ca != cb {
            return ca > cb;
        }
    }
    la > lb
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_explorer_tab
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw_explorer_tab(c: &mut Console, lay: &Layout, exp: &ExplorerState) {
    let fw = lay.fw;
    let _fh = lay.fh; // reservado para uso futuro
    let cw = lay.font_w; // 8
    let ch = lay.font_h; // 8
    let y0 = lay.content_y;
    let bot = lay.bottom_y;

    // Fondo completo hasta bottom_y
    c.fill_rect(0, y0, fw, bot.saturating_sub(y0), ExpPal::BG);

    // ═══════════════════════════════════════════════════════════════════
    // BREADCRUMB (barra de ruta) — misma paleta que chrome HDR
    // ═══════════════════════════════════════════════════════════════════
    c.fill_rect(0, y0, fw, HDR_H, Color::new(0x04, 0x0B, 0x18));
    c.hline(0, y0 + HDR_H - 1, fw, Color::new(0x18, 0x2C, 0x4A));

    // Icono de disco
    c.write_at("[HDD]", 6, y0 + (HDR_H - ch) / 2, ExpPal::GOLD);

    // Ruta — breadcrumbs separados por " > "
    let mut bx = 6 + 6 * cw;
    for i in 0..exp.path_depth {
        let node = &exp.path_stack[i];
        let name = node.name_str();
        let is_last = i + 1 == exp.path_depth;
        let fg = if is_last {
            Color::WHITE
        } else {
            ExpPal::TEXT_DIM
        };
        c.write_at(name, bx, y0 + (HDR_H - ch) / 2, fg);
        bx += name.len() * cw;
        if !is_last {
            c.write_at(" > ", bx, y0 + (HDR_H - ch) / 2, ExpPal::TEXT_DIM);
            bx += 3 * cw;
        }
    }

    // Atajos a la derecha
    let hint = "N=Dir  F=File  D=Eliminar  Tab=VFS  F5=Reload";
    c.write_at(
        hint,
        fw.saturating_sub(hint.len() * cw + 6),
        y0 + (HDR_H - ch) / 2,
        Color::new(0x30, 0x50, 0x80),
    );

    // ═══════════════════════════════════════════════════════════════════
    // LAYOUT INTERNO — calcular anchuras según show_vfs
    // ═══════════════════════════════════════════════════════════════════
    let content_y = y0 + HDR_H;
    let status_y = bot.saturating_sub(STATUS_H);
    let preview_y = status_y.saturating_sub(PREVIEW_H);
    let list_area_h = preview_y.saturating_sub(content_y + COL_HDR_H);
    let visible = (list_area_h / ROW_H).max(1);

    // Panel VFS sidebar izquierdo (paths del sistema)
    const VFS_W: usize = 128;
    let vfs_w = if exp.show_vfs { VFS_W } else { 0 };
    let tree_x = vfs_w;

    // ── VFS Sidebar ──────────────────────────────────────────────────
    if exp.show_vfs {
        c.fill_rect(
            0,
            content_y,
            vfs_w,
            preview_y.saturating_sub(content_y),
            Color::new(0x03, 0x07, 0x10),
        );
        c.vline(
            vfs_w,
            content_y,
            preview_y.saturating_sub(content_y),
            ExpPal::SEP_BRIGHT,
        );

        // Cabecera VFS
        c.fill_rect(0, content_y, vfs_w, COL_HDR_H, Color::new(0x06, 0x10, 0x22));
        c.write_at(
            "VFS",
            6,
            content_y + (COL_HDR_H - ch) / 2,
            Color::new(0xFF, 0xD7, 0x00),
        );
        c.hline(0, content_y + COL_HDR_H - 1, vfs_w, ExpPal::BORDER);

        // Entradas VFS
        let vfs_rows = ((preview_y.saturating_sub(content_y + COL_HDR_H)) / ROW_H).max(1);
        for (vi, entry) in VFS_TREE.iter().enumerate().take(vfs_rows) {
            let vy = content_y + COL_HDR_H + vi * ROW_H;
            let is_sel = vi == exp.vfs_sel;
            if is_sel {
                c.fill_rect(0, vy, vfs_w, ROW_H, Color::new(0x12, 0x28, 0x4A));
                c.fill_rect(0, vy, 2, ROW_H, Color::new(0xFF, 0xD7, 0x00));
            }
            let icon_fg = if entry.is_user {
                Color::new(0x44, 0xCC, 0x88)
            } else {
                Color::new(0x66, 0x99, 0xDD)
            };
            let lbl_fg = if is_sel {
                Color::WHITE
            } else {
                ExpPal::TREE_FG
            };
            let max_c = (vfs_w.saturating_sub(42)) / cw;
            let lbl = if entry.label.len() > max_c {
                &entry.label[..max_c]
            } else {
                entry.label
            };
            c.write_at(entry.icon, 4, vy + (ROW_H - ch) / 2, icon_fg);
            c.write_at(lbl, 4 + 4 * cw, vy + (ROW_H - ch) / 2, lbl_fg);
        }
    }

    // ── Panel árbol de ruta (derecho del VFS, izquierdo de la lista) ──
    let tree_w_used = TREE_W;
    let list_left = tree_x + tree_w_used + 1;

    c.fill_rect(
        tree_x,
        content_y,
        tree_w_used,
        preview_y.saturating_sub(content_y),
        ExpPal::PANEL_L_BG,
    );
    c.vline(
        tree_x + tree_w_used,
        content_y,
        preview_y.saturating_sub(content_y),
        ExpPal::SEP_BRIGHT,
    );

    // Cabecera árbol
    c.fill_rect(
        tree_x,
        content_y,
        tree_w_used,
        COL_HDR_H,
        ExpPal::COL_HDR_BG,
    );
    c.write_at(
        "Árbol",
        tree_x + 6,
        content_y + (COL_HDR_H - ch) / 2,
        ExpPal::COL_HDR_FG,
    );
    c.hline(
        tree_x,
        content_y + COL_HDR_H - 1,
        tree_w_used,
        ExpPal::BORDER,
    );

    // Dibuja la pila de rutas
    let tree_rows_nav = ((preview_y.saturating_sub(content_y + COL_HDR_H)) / ROW_H).max(1);
    let start_depth = if exp.path_depth > tree_rows_nav {
        exp.path_depth - tree_rows_nav
    } else {
        0
    };
    for i in start_depth..exp.path_depth {
        let node = &exp.path_stack[i];
        let row = i - start_depth;
        let ty = content_y + COL_HDR_H + row * ROW_H;
        let is_cur = i + 1 == exp.path_depth;
        let indent = i * 8;
        if is_cur {
            c.fill_rect(tree_x, ty, tree_w_used, ROW_H, ExpPal::ROW_SEL);
            c.fill_rect(tree_x, ty, 2, ROW_H, ExpPal::TREE_SEL);
        }
        let name = node.name_str();
        let max_c = (tree_w_used.saturating_sub(indent + 8)) / cw;
        let disp = if name.len() > max_c {
            &name[..max_c]
        } else {
            name
        };
        let fg = if is_cur {
            ExpPal::TREE_SEL
        } else {
            ExpPal::TREE_FG
        };
        if i > 0 {
            c.write_at(
                "+-",
                tree_x + indent + 4,
                ty + (ROW_H - ch) / 2,
                ExpPal::BORDER,
            );
        }
        c.write_at(
            disp,
            tree_x + indent + (if i > 0 { 20 } else { 6 }),
            ty + (ROW_H - ch) / 2,
            fg,
        );
    }

    // Panel lista (derecho del árbol)
    let list_x = list_left;
    let list_w = fw.saturating_sub(list_x + SCROLLBAR_W);

    // Cabecera de columnas
    c.fill_rect(list_x, content_y, list_w, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.write_at(
        "Nombre",
        list_x + 38,
        content_y + (COL_HDR_H - ch) / 2,
        ExpPal::COL_HDR_FG,
    );
    let size_col_x = fw.saturating_sub(SCROLLBAR_W + 80);
    let type_col_x = size_col_x.saturating_sub(52);
    c.write_at(
        "Tipo",
        type_col_x,
        content_y + (COL_HDR_H - ch) / 2,
        ExpPal::COL_HDR_FG,
    );
    c.write_at(
        "Tamaño",
        size_col_x,
        content_y + (COL_HDR_H - ch) / 2,
        ExpPal::COL_HDR_FG,
    );
    c.hline(list_x, content_y + COL_HDR_H - 1, list_w, ExpPal::BORDER);

    // Scrollbar background
    let sb_x = fw.saturating_sub(SCROLLBAR_W);
    c.fill_rect(
        sb_x,
        content_y + COL_HDR_H,
        SCROLLBAR_W,
        list_area_h,
        ExpPal::SCROLLBAR_BG,
    );

    // Calcular scroll
    let scroll = compute_scroll(exp.scroll, exp.selected, visible);

    // Filas de la lista
    for vis in 0..visible {
        let idx = scroll + vis;
        if idx >= exp.entry_count {
            break;
        }
        let py = content_y + COL_HDR_H + vis * ROW_H;
        let is_sel = idx == exp.selected;

        let base_bg = if vis % 2 == 0 {
            ExpPal::ROW_EVEN
        } else {
            ExpPal::ROW_ODD
        };
        let bg = if is_sel { ExpPal::ROW_SEL } else { base_bg };
        c.fill_rect(list_x, py, list_w, ROW_H, bg);

        // Línea de selección (borde izquierdo dorado)
        if is_sel {
            c.fill_rect(list_x, py, 3, ROW_H, ExpPal::GOLD);
        }

        if let Some(entry) = &exp.entries[idx] {
            let name = entry.name_str();
            let kind = file_kind(name, entry.is_dir);
            let (icon_str, icon_col) = kind_icon(kind);
            let name_col = if is_sel {
                Color::WHITE
            } else {
                kind_name_color(kind)
            };

            let text_y = py + (ROW_H - ch) / 2;

            // Icono
            c.write_at(icon_str, list_x + 4, text_y, icon_col);

            // Nombre (con truncado)
            let max_name_chars = type_col_x.saturating_sub(list_x + 44) / cw;
            let name_disp = if name.len() > max_name_chars && max_name_chars > 3 {
                &name[..max_name_chars - 1] // deja espacio para '…'
            } else {
                name
            };
            c.write_at(name_disp, list_x + 44, text_y, name_col);
            if name.len() > max_name_chars && max_name_chars > 3 {
                c.write_at(
                    "~",
                    list_x + 44 + max_name_chars * cw - cw,
                    text_y,
                    Color::new(0x44, 0x55, 0x66),
                );
            }

            // Tipo
            let type_str = if entry.is_dir { "DIR" } else { file_ext(name) };
            c.write_at(type_str, type_col_x, text_y, ExpPal::TYPE_FG);

            // Tamaño
            if !entry.is_dir {
                let mut sb = [0u8; 16];
                let ss = fmt_size(entry.size, &mut sb);
                let size_x = fw.saturating_sub(SCROLLBAR_W + ss.len() * cw + 4);
                c.write_at(
                    ss,
                    size_x,
                    text_y,
                    if is_sel {
                        ExpPal::SIZE_FG
                    } else {
                        ExpPal::SIZE_FG
                    },
                );
            } else {
                c.write_at("—", size_col_x + 12, text_y, ExpPal::TEXT_DIM);
            }
        }
    }

    // Scrollbar thumb
    if exp.entry_count > visible && visible > 0 {
        let thumb_h = (list_area_h * visible / exp.entry_count)
            .max(6)
            .min(list_area_h);
        let thumb_y = content_y
            + COL_HDR_H
            + (scroll * list_area_h / exp.entry_count).min(list_area_h.saturating_sub(thumb_h));
        c.fill_rounded(
            sb_x + 1,
            thumb_y,
            SCROLLBAR_W - 2,
            thumb_h,
            2,
            ExpPal::SCROLLBAR_FG,
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // PANEL PREVIEW
    // ═══════════════════════════════════════════════════════════════════
    c.hline(0, preview_y, fw, ExpPal::SEP_BRIGHT);
    c.fill_rect(0, preview_y + 1, fw, PREVIEW_H - 1, ExpPal::PREVIEW_BG);

    // Cabecera preview
    let prev_hdr_y = preview_y + 1;
    c.fill_rect(0, prev_hdr_y, fw, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.hline(0, prev_hdr_y + COL_HDR_H - 1, fw, ExpPal::BORDER);
    c.write_at(
        "Vista previa",
        8,
        prev_hdr_y + (COL_HDR_H - ch) / 2,
        ExpPal::PREVIEW_HDR,
    );

    if exp.preview_len > 0 {
        let prev_name = core::str::from_utf8(&exp.preview_name[..exp.preview_nlen]).unwrap_or("");
        let name_x = 8 + 14 * cw;
        c.write_at(
            "—",
            name_x - cw,
            prev_hdr_y + (COL_HDR_H - ch) / 2,
            ExpPal::TEXT_DIM,
        );
        c.write_at(
            prev_name,
            name_x,
            prev_hdr_y + (COL_HDR_H - ch) / 2,
            Color::WHITE,
        );

        // Bytes totales
        let mut sb = [0u8; 16];
        let ss = fmt_size(exp.preview_len as u32, &mut sb);
        c.write_at(
            ss,
            fw.saturating_sub(ss.len() * cw + 8),
            prev_hdr_y + (COL_HDR_H - ch) / 2,
            ExpPal::SIZE_FG,
        );

        // Contenido de la preview (texto)
        let data = &exp.preview[..exp.preview_len];
        let mut ls = 0usize;
        let mut ln = 0usize;
        let text_y0 = prev_hdr_y + COL_HDR_H + 2;
        for i in 0..=data.len() {
            if (i == data.len() || data[i] == b'\n') && ln < PREVIEW_LINES {
                let bytes = &data[ls..i];
                let mc = fw.saturating_sub(16) / cw;
                let disp = &bytes[..bytes.len().min(mc)];
                if let Ok(s) = core::str::from_utf8(disp) {
                    c.write_at(s, 8, text_y0 + ln * (ch + 2), ExpPal::PREVIEW_FG);
                }
                ln += 1;
                ls = i + 1;
            }
        }
    } else if exp.entry_count > 0 {
        c.write_at(
            "Selecciona un archivo de texto para vista previa.",
            8,
            prev_hdr_y + COL_HDR_H + 4,
            ExpPal::TEXT_DIM,
        );
    } else {
        c.write_at(
            "Directorio vacío.",
            8,
            prev_hdr_y + COL_HDR_H + 4,
            ExpPal::TEXT_DIM,
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // STATUS BAR / INPUT BOX
    // ═══════════════════════════════════════════════════════════════════

    let in_input = exp.input.is_active();
    let st_bg = if in_input && exp.input.mode == InputMode::Delete {
        INPUT_BG_DELETE
    } else if in_input {
        INPUT_BG
    } else if exp.status_ok {
        ExpPal::STATUS_BG
    } else {
        ExpPal::STATUS_ERR
    };

    c.fill_rect(0, status_y, fw, STATUS_H, st_bg);
    c.hline(0, status_y, fw, Color::new(0x00, 0x55, 0xBB));

    if in_input {
        // draw_input_overlay unificado — mismo código que el IDE
        draw_input_overlay(c, &exp.input, 8, status_y, fw, STATUS_H, cw, ch);
    } else {
        let sy_text = status_y + (STATUS_H - ch) / 2;
        let status = core::str::from_utf8(&exp.status[..exp.status_len]).unwrap_or("");
        c.write_at(status, 8, sy_text, Color::WHITE);

        let mut cnt_buf = [0u8; 32];
        let mut cp = 0usize;
        let mut tmp = [0u8; 8];
        let ns = fmt_usize_local(exp.entry_count, &mut tmp);
        for b in ns.bytes() {
            if cp < 24 {
                cnt_buf[cp] = b;
                cp += 1;
            }
        }
        for b in b" elementos" {
            if cp < 32 {
                cnt_buf[cp] = *b;
                cp += 1;
            }
        }
        let cnt_str = core::str::from_utf8(&cnt_buf[..cp]).unwrap_or("");
        c.write_at(
            cnt_str,
            fw.saturating_sub(cnt_str.len() * cw + 8),
            sy_text,
            Color::WHITE,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn compute_scroll(prev_scroll: usize, selected: usize, visible: usize) -> usize {
    if selected < prev_scroll {
        selected
    } else if visible > 0 && selected >= prev_scroll + visible {
        selected + 1 - visible
    } else {
        prev_scroll
    }
}

fn file_ext(name: &str) -> &'static str {
    if name.ends_with(".rs") {
        "RUST"
    } else if name.ends_with(".c") {
        "C   "
    } else if name.ends_with(".h") {
        "HDR "
    } else if name.ends_with(".asm") {
        "ASM "
    } else if name.ends_with(".s") {
        "ASM "
    } else if name.ends_with(".txt") {
        "TXT "
    } else if name.ends_with(".md") {
        "MD  "
    } else if name.ends_with(".bin") {
        "BIN "
    } else if name.ends_with(".elf") {
        "ELF "
    } else if name.ends_with(".bmp") {
        "BMP "
    } else if name.ends_with(".toml") {
        "TOML"
    } else {
        "    "
    }
}

fn fmt_size(bytes: u32, buf: &mut [u8; 16]) -> &str {
    let mut p = 0usize;
    let mut tmp = [0u8; 8];
    if bytes < 1024 {
        let s = fmt_usize_local(bytes as usize, &mut tmp);
        for b in s.bytes() {
            if p < 10 {
                buf[p] = b;
                p += 1;
            }
        }
        for b in b" B" {
            if p < 14 {
                buf[p] = *b;
                p += 1;
            }
        }
    } else if bytes < 1024 * 1024 {
        let s = fmt_usize_local((bytes / 1024) as usize, &mut tmp);
        for b in s.bytes() {
            if p < 10 {
                buf[p] = b;
                p += 1;
            }
        }
        for b in b" KB" {
            if p < 14 {
                buf[p] = *b;
                p += 1;
            }
        }
    } else {
        let s = fmt_usize_local((bytes / (1024 * 1024)) as usize, &mut tmp);
        for b in s.bytes() {
            if p < 10 {
                buf[p] = b;
                p += 1;
            }
        }
        for b in b" MB" {
            if p < 14 {
                buf[p] = *b;
                p += 1;
            }
        }
    }
    core::str::from_utf8(&buf[..p]).unwrap_or("?")
}

fn fmt_usize_local(mut n: usize, buf: &mut [u8]) -> &str {
    let mut i = buf.len();
    if i == 0 {
        return "";
    }
    if n == 0 {
        buf[i - 1] = b'0';
        return core::str::from_utf8(&buf[i - 1..]).unwrap_or("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}
