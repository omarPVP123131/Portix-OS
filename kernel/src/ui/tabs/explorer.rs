// ui/tabs/explorer.rs — PORTIX Kernel v0.8.0
//
// REDISEÑO COMPLETO:
//  - Paleta desaturada (VS Code File Explorer-inspired)
//  - Barra de herramientas con pestañas: [Archivos] [Marcadores] [Recientes]
//  - Menú contextual con clic derecho (3 zonas: VFS, árbol, lista)
//  - Atajos ocultos → overlay con F1 o botón [?]
//  - Headers/footers modernos, sin "pixel art azul"
//  - Driver de disco primario + fallback automático
//  - Layout más limpio y menos recargado

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::drivers::storage::fat32::{DirEntryInfo, Fat32Volume};
use crate::drivers::storage::vfs::VFS_TREE;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::ui::input::{draw_input_overlay, InputBox, InputMode, INPUT_BG, INPUT_BG_DELETE, INPUT_MAX};

// ─────────────────────────────────────────────────────────────────────────────
// Paleta — desaturada, moderna
// ─────────────────────────────────────────────────────────────────────────────

pub struct ExpPal;
impl ExpPal {
    // Fondos
    pub const BG:          Color = Color::new(0x1E, 0x1E, 0x1E);
    pub const SIDEBAR_BG:  Color = Color::new(0x17, 0x17, 0x17);
    pub const PANEL_BG:    Color = Color::new(0x1E, 0x1E, 0x1E);
    pub const HDR_BG:      Color = Color::new(0x33, 0x33, 0x33);
    pub const TOOLBAR_BG:  Color = Color::new(0x25, 0x25, 0x25);
    pub const COL_HDR_BG:  Color = Color::new(0x25, 0x25, 0x25);
    pub const ROW_ODD:     Color = Color::new(0x1E, 0x1E, 0x1E);
    pub const ROW_EVEN:    Color = Color::new(0x22, 0x22, 0x22);
    pub const ROW_SEL:     Color = Color::new(0x09, 0x44, 0x77);
    pub const ROW_HOV:     Color = Color::new(0x2A, 0x2A, 0x2A);
    pub const CONTEXT_BG:  Color = Color::new(0x25, 0x25, 0x25);
    pub const CONTEXT_BOR: Color = Color::new(0x45, 0x45, 0x45);
    pub const CONTEXT_HOV: Color = Color::new(0x04, 0x44, 0x7A);
    pub const CONTEXT_SEP: Color = Color::new(0x3A, 0x3A, 0x3A);
    pub const PREVIEW_BG:  Color = Color::new(0x19, 0x19, 0x19);
    pub const STATUS_BG:   Color = Color::new(0x00, 0x7A, 0xCC);
    pub const STATUS_ERR:  Color = Color::new(0xA1, 0x26, 0x0C);
    pub const OVERLAY_BG:  Color = Color::new(0x25, 0x25, 0x25);
    // Texto
    pub const TEXT:        Color = Color::new(0xCC, 0xCC, 0xCC);
    pub const TEXT_DIM:    Color = Color::new(0x66, 0x6E, 0x7A);
    pub const TEXT_SEL:    Color = Color::new(0xFF, 0xFF, 0xFF);
    pub const BORDER:      Color = Color::new(0x2D, 0x2D, 0x2D);
    pub const BORDER_BRIG: Color = Color::new(0x45, 0x45, 0x45);
    // Archivos
    pub const DIR_FG:      Color = Color::new(0xE8, 0xC0, 0x6A);
    pub const DIR_ICON:    Color = Color::new(0xFF, 0xBF, 0x00);
    pub const FILE_FG:     Color = Color::new(0x9C, 0xBE, 0xE8);
    pub const FILE_ICON:   Color = Color::new(0x60, 0x90, 0xC0);
    pub const FILE_RS:     Color = Color::new(0xDE, 0x6A, 0x40);
    pub const FILE_C:      Color = Color::new(0x44, 0x99, 0xFF);
    pub const FILE_ASM:    Color = Color::new(0xCC, 0xAA, 0x00);
    pub const FILE_IMG:    Color = Color::new(0x66, 0xCC, 0x88);
    pub const SIZE_FG:     Color = Color::new(0x80, 0x80, 0x80);
    pub const TYPE_FG:     Color = Color::new(0x66, 0x6E, 0x7A);
    pub const ACCENT:      Color = Color::new(0x00, 0x7A, 0xCC);
    pub const ACCENT2:     Color = Color::new(0x00, 0xBF, 0xFF);
    pub const GOLD:        Color = Color::new(0xFF, 0xBF, 0x00);
    pub const VFS_FG:      Color = Color::new(0x9C, 0xBE, 0xE8);
    pub const VFS_SEL:     Color = Color::new(0xFF, 0xFF, 0xFF);
    pub const PREVIEW_FG:  Color = Color::new(0x80, 0xA8, 0xCC);
    pub const SCR_BG:      Color = Color::new(0x20, 0x20, 0x20);
    pub const SCR_FG:      Color = Color::new(0x40, 0x40, 0x40);
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout interno
// ─────────────────────────────────────────────────────────────────────────────

pub const TOOLBAR_H:  usize = 28;  // barra de pestañas/herramientas
const HDR_H:      usize = 22;  // breadcrumb
const COL_HDR_H:  usize = 18;  // cabecera de columnas
const PREVIEW_H:  usize = 76;  // panel preview
const STATUS_H:   usize = 18;  // status bar
const SIDEBAR_W:  usize = 120; // panel VFS izquierdo
const TREE_W:     usize = 150; // árbol de rutas
const SCR_W:      usize = 8;   // scrollbar
const ROW_H:      usize = 16;  // altura fila

const MAX_ENTRIES:    usize = 256;
const MAX_PATH_DEPTH: usize = 32;
const PREVIEW_BYTES:  usize = 2048;
const PREVIEW_LINES:  usize = 4;

const CONTEXT_ITEM_H: usize = 18;

// ─────────────────────────────────────────────────────────────────────────────
// Vista del explorer
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum ExplorerView { Files, Bookmarks, Recent }

// ─────────────────────────────────────────────────────────────────────────────
// Menú contextual
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum ContextZone { None, Sidebar, Tree, FileList, EmptyArea }

#[derive(Clone, Copy, PartialEq)]
pub enum ContextAction {
    None, Separator,
    Open, OpenWithIde,
    NewFolder, NewFile,
    Delete, Rename,
    CopyPath,
    AddBookmark,
    Refresh,
    Properties,
}

#[derive(Clone, Copy)]
pub struct ContextItem { pub label: &'static str, pub action: ContextAction }
impl ContextItem {
    const fn new(l: &'static str, a: ContextAction) -> Self { ContextItem { label: l, action: a } }
    const fn sep() -> Self { ContextItem { label: "─────────────────", action: ContextAction::Separator } }
}

pub struct ContextMenu {
    pub visible:    bool,
    pub x:          usize,
    pub y:          usize,
    pub zone:       ContextZone,
    pub items:      [ContextItem; 10],
    pub item_count: usize,
    pub hovered:    usize,
}

impl ContextMenu {
    pub const fn new() -> Self {
        ContextMenu {
            visible: false, x: 0, y: 0,
            zone: ContextZone::None,
            items: [ContextItem { label: "", action: ContextAction::None }; 10],
            item_count: 0, hovered: usize::MAX,
        }
    }

    fn show_for_zone(&mut self, x: usize, y: usize, zone: ContextZone, has_file: bool) {
        self.visible = true; self.x = x; self.y = y; self.zone = zone; self.item_count = 0; self.hovered = usize::MAX;
        match zone {
            ContextZone::Sidebar => {
                self.push(ContextItem::new("Abrir carpeta", ContextAction::Open));
                self.push(ContextItem::sep());
                self.push(ContextItem::new("Agregar marcador", ContextAction::AddBookmark));
                self.push(ContextItem::new("Copiar ruta", ContextAction::CopyPath));
            }
            ContextZone::Tree => {
                self.push(ContextItem::new("Abrir", ContextAction::Open));
                self.push(ContextItem::sep());
                self.push(ContextItem::new("Nueva carpeta", ContextAction::NewFolder));
                self.push(ContextItem::new("Actualizar", ContextAction::Refresh));
            }
            ContextZone::FileList if has_file => {
                self.push(ContextItem::new("Abrir", ContextAction::Open));
                self.push(ContextItem::new("Abrir con IDE", ContextAction::OpenWithIde));
                self.push(ContextItem::sep());
                self.push(ContextItem::new("Renombrar", ContextAction::Rename));
                self.push(ContextItem::new("Eliminar", ContextAction::Delete));
                self.push(ContextItem::sep());
                self.push(ContextItem::new("Copiar ruta", ContextAction::CopyPath));
                self.push(ContextItem::new("Propiedades", ContextAction::Properties));
            }
            ContextZone::EmptyArea | _ => {
                self.push(ContextItem::new("Nueva carpeta", ContextAction::NewFolder));
                self.push(ContextItem::new("Nuevo archivo", ContextAction::NewFile));
                self.push(ContextItem::sep());
                self.push(ContextItem::new("Actualizar", ContextAction::Refresh));
            }
        }
    }

    fn push(&mut self, item: ContextItem) { if self.item_count < 10 { self.items[self.item_count] = item; self.item_count += 1; } }
    pub fn close(&mut self) { self.visible = false; self.item_count = 0; }

pub fn height(&self) -> usize { self.item_count * CONTEXT_ITEM_H + 8 }
pub fn width(&self, cw: usize) -> usize { // <--- Ahora es accesible desde main.rs
    let max_l = self.items[..self.item_count].iter().map(|it| it.label.len()).max().unwrap_or(10);
    (max_l + 4) * cw + 16
}
}

// ─────────────────────────────────────────────────────────────────────────────
// PathNode
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PathNode {
    pub name:     [u8; 256],
    pub name_len: usize,
    pub cluster:  u32,
}
impl PathNode {
    pub const fn root(cluster: u32) -> Self {
        let mut name = [0u8; 256]; name[0] = b'/';
        PathNode { name, name_len: 1, cluster }
    }
    pub fn name_str(&self) -> &str { core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?") }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tipos de archivo
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum FileKind { Dir, Rust, C, Asm, Text, Image, Binary, Other }

fn file_kind(name: &str, is_dir: bool) -> FileKind {
    if is_dir { return FileKind::Dir; }
    if name.ends_with(".rs") { FileKind::Rust }
    else if name.ends_with(".c") || name.ends_with(".h") { FileKind::C }
    else if name.ends_with(".asm") || name.ends_with(".s") { FileKind::Asm }
    else if name.ends_with(".txt") || name.ends_with(".md") { FileKind::Text }
    else if name.ends_with(".bmp") || name.ends_with(".png") { FileKind::Image }
    else if name.ends_with(".bin") || name.ends_with(".elf") { FileKind::Binary }
    else { FileKind::Other }
}

fn kind_icon(k: FileKind) -> (&'static str, Color) {
    match k {
        FileKind::Dir    => ("▶", ExpPal::DIR_ICON),
        FileKind::Rust   => ("⬡", ExpPal::FILE_RS),
        FileKind::C      => ("◈", ExpPal::FILE_C),
        FileKind::Asm    => ("⊞", ExpPal::FILE_ASM),
        FileKind::Text   => ("≡", ExpPal::FILE_FG),
        FileKind::Image  => ("⊡", ExpPal::FILE_IMG),
        FileKind::Binary => ("⊟", ExpPal::TYPE_FG),
        FileKind::Other  => ("◦", ExpPal::TYPE_FG),
    }
}

// fallback ASCII para sistemas sin unicode en framebuffer
fn kind_icon_ascii(k: FileKind) -> (&'static str, Color) {
    match k {
        FileKind::Dir    => ("[D]", ExpPal::DIR_ICON),
        FileKind::Rust   => ("[rs]", ExpPal::FILE_RS),
        FileKind::C      => ("[ c]", ExpPal::FILE_C),
        FileKind::Asm    => ("[as]", ExpPal::FILE_ASM),
        FileKind::Text   => ("[tx]", ExpPal::FILE_FG),
        FileKind::Image  => ("[im]", ExpPal::FILE_IMG),
        FileKind::Binary => ("[bi]", ExpPal::TYPE_FG),
        FileKind::Other  => ("[  ]", ExpPal::TYPE_FG),
    }
}

fn kind_fg(k: FileKind, selected: bool) -> Color {
    if selected { return ExpPal::TEXT_SEL; }
    match k {
        FileKind::Dir  => ExpPal::DIR_FG,
        FileKind::Rust => Color::new(0xE8, 0xB0, 0x90),
        FileKind::C    => Color::new(0x88, 0xCC, 0xFF),
        FileKind::Asm  => Color::new(0xDD, 0xCC, 0x88),
        _              => ExpPal::FILE_FG,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bookmarks (marcadores simples)
// ─────────────────────────────────────────────────────────────────────────────

const MAX_BOOKMARKS: usize = 8;

#[derive(Clone)]
pub struct Bookmark {
    pub path:     [u8; 256],
    pub path_len: usize,
    pub cluster:  u32,
}
impl Bookmark {
    const fn empty() -> Self { Bookmark { path: [0u8; 256], path_len: 0, cluster: 0 } }
    pub fn path_str(&self) -> &str { core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("?") }
}

// ─────────────────────────────────────────────────────────────────────────────
// ExplorerState
// ─────────────────────────────────────────────────────────────────────────────

pub struct ExplorerState {
    // Navegación
    pub path_stack: [PathNode; MAX_PATH_DEPTH],
    pub path_depth: usize,
    pub entries:    [Option<DirEntryInfo>; MAX_ENTRIES],
    pub entry_count:usize,
    pub selected:   usize,
    pub scroll:     usize,

    // Preview
    pub preview:      [u8; PREVIEW_BYTES],
    pub preview_len:  usize,
    pub preview_name: [u8; 256],
    pub preview_nlen: usize,

    // Status
    pub status:     [u8; 80],
    pub status_len: usize,
    pub status_ok:  bool,

    // Señal de apertura en IDE
    pub open_request:   bool,
    pub open_cluster:   u32,
    pub open_name:      [u8; 256],
    pub open_name_len:  usize,
    pub open_size:      u32,
    pub needs_refresh:  bool,

    // Input inline
    pub input: InputBox,

    // VFS sidebar
    pub vfs_sel:  usize,
    pub show_vfs: bool,

    // Vista actual (toolbar tabs)
    pub view:    ExplorerView,

    // Menú contextual
    pub context: ContextMenu,

    // Marcadores
    pub bookmarks:      [Bookmark; MAX_BOOKMARKS],
    pub bookmark_count: usize,

    // Recientes (últimas rutas abiertas)
    pub recent:      [[u8; 256]; 8],
    pub recent_lens: [usize; 8],
    pub recent_count:usize,

    // Ayuda
    pub show_help: bool,
}

impl ExplorerState {
    pub fn new(root_cluster: u32) -> Self {
        const NONE_ENTRY: Option<DirEntryInfo> = None;
        const ROOT_NODE:  PathNode = PathNode::root(0);
        let mut s = ExplorerState {
            path_stack:     [ROOT_NODE; MAX_PATH_DEPTH],
            path_depth:     1,
            entries:        [NONE_ENTRY; MAX_ENTRIES],
            entry_count:    0,
            selected:       0,
            scroll:         0,
            preview:        [0u8; PREVIEW_BYTES],
            preview_len:    0,
            preview_name:   [0u8; 256],
            preview_nlen:   0,
            status:         [0u8; 80],
            status_len:     0,
            status_ok:      true,
            open_request:   false,
            open_cluster:   0,
            open_name:      [0u8; 256],
            open_name_len:  0,
            open_size:      0,
            needs_refresh:  true,
            input:          InputBox::new(),
            vfs_sel:        0,
            show_vfs:       true,
            view:           ExplorerView::Files,
            context:        ContextMenu::new(),
            bookmarks:      [const { Bookmark::empty() }; MAX_BOOKMARKS],
            bookmark_count: 0,
            recent:         [[0u8; 256]; 8],
            recent_lens:    [0usize; 8],
            recent_count:   0,
            show_help:      false,
        };
        s.path_stack[0] = PathNode::root(root_cluster);
        s
    }

    pub fn current_cluster(&self) -> u32 { self.path_stack[self.path_depth.saturating_sub(1)].cluster }

    pub fn set_status(&mut self, msg: &str, ok: bool) {
        let n = msg.len().min(80);
        self.status[..n].copy_from_slice(&msg.as_bytes()[..n]);
        self.status_len = n; self.status_ok = ok;
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
            if name == "." || name == ".." { return; }
            if count < MAX_ENTRIES { entries_ref[count] = Some(e.clone()); count += 1; }
        });
        self.entry_count = count;
        sort_entries(&mut self.entries, count);
        if self.selected >= count && count > 0 { self.selected = count - 1; }
        self.needs_refresh = false;
        self.set_status("Directorio cargado", true);
    }

    pub fn load_preview(&mut self, vol: &Fat32Volume) {
        if let Some(entry) = self.entries[self.selected].as_ref() {
            if entry.is_dir { self.preview_len = 0; return; }
            let mut n = [0u8; 256];
            n[..entry.name_len].copy_from_slice(&entry.name[..entry.name_len]);
            self.preview_name = n; self.preview_nlen = entry.name_len;
            let cloned = entry.clone();
            self.preview_len = vol.read_file(&cloned, &mut self.preview).unwrap_or(0);
        } else { self.preview_len = 0; }
    }

    pub fn selected_entry(&self) -> Option<&DirEntryInfo> {
        if self.selected < self.entry_count { self.entries[self.selected].as_ref() } else { None }
    }

    fn try_enter_dir(&mut self) -> bool {
        let (is_dir, cluster, name_len, name) = if let Some(e) = self.selected_entry() {
            let mut n = [0u8; 256]; n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.is_dir, e.cluster, e.name_len, n)
        } else { return false; };
        if is_dir && self.path_depth < MAX_PATH_DEPTH {
            self.path_stack[self.path_depth] = PathNode { name, name_len, cluster };
            self.path_depth += 1; self.selected = 0; self.scroll = 0;
            self.needs_refresh = true; self.preview_len = 0; true
        } else { false }
    }

    fn try_open_file(&mut self) -> bool {
        let (cluster, size, name_len, name) = if let Some(e) = self.selected_entry() {
            if e.is_dir { return false; }
            let mut n = [0u8; 256]; n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.cluster, e.size, e.name_len, n)
        } else { return false; };
        self.open_request = true; self.open_cluster = cluster; self.open_size = size;
        self.open_name = name; self.open_name_len = name_len;
        // Agregar a recientes
        self.push_recent(&name[..name_len]);
        true
    }

    fn push_recent(&mut self, name: &[u8]) {
        if self.recent_count < 8 {
            let n = name.len().min(255);
            self.recent[self.recent_count][..n].copy_from_slice(&name[..n]);
            self.recent_lens[self.recent_count] = n;
            self.recent_count += 1;
        }
    }

    pub fn go_up(&mut self) {
        if self.path_depth > 1 {
            self.path_depth -= 1; self.selected = 0; self.scroll = 0;
            self.needs_refresh = true; self.preview_len = 0;
        }
    }

    /// Maneja clic derecho — abre menú contextual en la zona correcta
    pub fn handle_right_click(&mut self, rx: usize, ry: usize, lay_cly: usize, fw: usize) {
        // Cerrar input/menú previo
        if self.input.is_active() { return; }
        self.context.close();

        // Calcular zonas (coincide con draw_explorer_tab)
       let content_y = lay_cly + TOOLBAR_H + HDR_H;
        let vfs_w = if self.show_vfs { SIDEBAR_W } else { 0 };
        let tree_x = vfs_w;
        let tree_end = tree_x + TREE_W;
        let list_x = tree_end + 1; // <--- Ahora la usaremos abajo

        if ry < content_y { return; } // clic en toolbar/header

        let has_file = self.selected_entry().map(|e| !e.is_dir).unwrap_or(false);

        let zone = if self.show_vfs && rx < vfs_w {
            ContextZone::Sidebar
        } else if rx >= tree_x && rx < tree_end { // Más preciso
            ContextZone::Tree
        } else if rx >= list_x && rx < fw.saturating_sub(SCR_W) { // <--- Uso de list_x
            if self.entry_count > 0 { ContextZone::FileList } else { ContextZone::EmptyArea }
        } else {
            return;
        };

        self.context.show_for_zone(rx, ry, zone, has_file);
    }

    /// Ejecuta la acción del menú contextual en el item clickeado
    pub fn execute_context(&mut self, item_idx: usize) -> bool {
        if item_idx >= self.context.item_count { self.context.close(); return false; }
        let action = self.context.items[item_idx].action;
        self.context.close();
        match action {
            ContextAction::Open           => { if !self.try_enter_dir() { self.try_open_file(); } true }
            ContextAction::OpenWithIde    => { self.try_open_file(); true }
            ContextAction::NewFolder      => { self.input.start(InputMode::NewDir, "nueva_carpeta"); self.set_status("Nombre de carpeta (Enter=OK):", true); true }
            ContextAction::NewFile        => { self.input.start(InputMode::NewFile, "nuevo.txt"); self.set_status("Nombre del archivo (Enter=OK):", true); true }
            ContextAction::Delete         => {
                let maybe = self.selected_entry().map(|e| (e.name, e.name_len));
                if let Some((n, nl)) = maybe {
                    let ns = core::str::from_utf8(&n[..nl.min(INPUT_MAX)]).unwrap_or("archivo");
                    self.input.start(InputMode::Delete, ns);
                    self.set_status("Eliminar (Enter=confirmar, Esc=cancelar):", false);
                }
                true
            }
            ContextAction::Rename         => { self.input.start(InputMode::NewFile, ""); self.set_status("Nuevo nombre (Enter=OK, Esc=cancelar):", true); true }
            ContextAction::AddBookmark    => { self.add_current_bookmark(); true }
            ContextAction::CopyPath       => { self.set_status("Ruta copiada (sin portapapeles en modo kernel)", true); true }
            ContextAction::Refresh        => { self.needs_refresh = true; true }
            ContextAction::Properties     => { self.show_properties(); true }
            ContextAction::Separator      => true,
            ContextAction::None           => false,
        }
    }

    fn add_current_bookmark(&mut self) {
        if self.bookmark_count >= MAX_BOOKMARKS { return; }
        let node = &self.path_stack[self.path_depth.saturating_sub(1)];
        let n = node.name_len.min(255);
        self.bookmarks[self.bookmark_count].path[..n].copy_from_slice(&node.name[..n]);
        self.bookmarks[self.bookmark_count].path_len = n;
        self.bookmarks[self.bookmark_count].cluster = node.cluster;
        self.bookmark_count += 1;
        self.set_status("Marcador agregado", true);
    }

    fn show_properties(&mut self) {
        if let Some(e) = self.selected_entry() {
            let name = e.name_str();
            let mut msg = [0u8; 80]; let mut mp = 0;
            for b in name.bytes() { if mp < 30 { msg[mp] = b; mp += 1; } }
            for b in b"  Tam:" { msg[mp] = *b; mp += 1; }
            let mut tb = [0u8; 16];
            let ss = fmt_size_local(e.size, &mut tb);
            for b in ss.bytes() { if mp < 78 { msg[mp] = b; mp += 1; } }
            self.set_status(core::str::from_utf8(&msg[..mp]).unwrap_or(""), true);
        }
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        // Cerrar help overlay
        if self.show_help { self.show_help = false; return true; }
        // Cerrar menú contextual
        if self.context.visible { self.context.close(); return true; }

        if self.input.is_active() {
            if let Some(confirmed) = self.input.feed(key) {
                let mode = self.input.mode;
                self.input.close();
                if confirmed {
                    match mode {
                        InputMode::NewDir  => { self.needs_refresh = true; self.set_status("Carpeta creada (pendiente FAT32)", true); }
                        InputMode::NewFile => { self.needs_refresh = true; self.set_status("Archivo creado (pendiente FAT32)", true); }
                        InputMode::Delete  => { self.needs_refresh = true; self.set_status("Eliminado (pendiente FAT32)", true); }
                        _ => {}
                    }
                } else { self.set_status("Cancelado", true); }
            }
            return true;
        }

        match key {
            Key::Up    => { if self.selected > 0 { self.selected -= 1; } self.clamp_scroll(0); true }
            Key::Down  => { if self.selected + 1 < self.entry_count { self.selected += 1; } self.clamp_scroll(0); true }
            Key::PageUp   => { self.selected = self.selected.saturating_sub(12); self.clamp_scroll(0); true }
            Key::PageDown => { self.selected = (self.selected + 12).min(self.entry_count.saturating_sub(1)); self.clamp_scroll(0); true }
            Key::Enter    => { if !self.try_enter_dir() { self.try_open_file(); } true }
            Key::Backspace => { self.go_up(); true }
            Key::F1       => { self.show_help = true; true }
            Key::F5       => { self.needs_refresh = true; true }
            Key::Char(b'n') | Key::Char(b'N') => { self.input.start(InputMode::NewDir, "nueva_carpeta"); self.set_status("Nombre de carpeta (Enter=OK, Esc=Cancelar):", true); true }
            Key::Char(b'f') | Key::Char(b'F') => { self.input.start(InputMode::NewFile, "nuevo.txt"); self.set_status("Nombre del archivo (Enter=OK, Esc=Cancelar):", true); true }
            Key::Char(b'd') | Key::Char(b'D') | Key::Delete => {
                let maybe = self.selected_entry().filter(|e| !e.is_dir).map(|e| (e.name, e.name_len));
                if let Some((n, nl)) = maybe {
                    let ns = core::str::from_utf8(&n[..nl.min(INPUT_MAX)]).unwrap_or("archivo");
                    self.input.start(InputMode::Delete, ns);
                    self.set_status("Eliminar (Enter=confirmar, Esc=cancelar):", false);
                }
                true
            }
            Key::Tab => {
                // Rotar vistas: Files → Bookmarks → Recent → Files
                self.view = match self.view { ExplorerView::Files => ExplorerView::Bookmarks, ExplorerView::Bookmarks => ExplorerView::Recent, ExplorerView::Recent => ExplorerView::Files };
                true
            }
            _ => false,
        }
    }

    fn clamp_scroll(&mut self, vis: usize) {
        if self.selected < self.scroll { self.scroll = self.selected; }
        if vis > 0 && self.selected >= self.scroll + vis { self.scroll = self.selected + 1 - vis; }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ordenación
// ─────────────────────────────────────────────────────────────────────────────

fn sort_entries(entries: &mut [Option<DirEntryInfo>; MAX_ENTRIES], count: usize) {
    for i in 0..count {
        for j in i + 1..count {
            let swap = match (&entries[i], &entries[j]) {
                (Some(a), Some(b)) => {
                    if a.is_dir && !b.is_dir { false } else if !a.is_dir && b.is_dir { true } else { name_gt(a, b) }
                }
                _ => false,
            };
            if swap { entries.swap(i, j); }
        }
    }
}

fn name_gt(a: &DirEntryInfo, b: &DirEntryInfo) -> bool {
    let la = a.name_len.min(16); let lb = b.name_len.min(16);
    for i in 0..la.min(lb) {
        let ca = a.name[i].to_ascii_lowercase(); let cb = b.name[i].to_ascii_lowercase();
        if ca != cb { return ca > cb; }
    }
    la > lb
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_explorer_tab
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw_explorer_tab(c: &mut Console, lay: &Layout, exp: &ExplorerState) {
    let fw  = lay.fw;
    let cw  = lay.font_w;
    let ch  = lay.font_h;
    let y0  = lay.content_y;
    let bot = lay.bottom_y;

    // Fondo
    c.fill_rect(0, y0, fw, bot.saturating_sub(y0), ExpPal::BG);

    // ═════════════════════════════════════════════════════════════════════════
    // TOOLBAR — pestañas de vista + botón ayuda
    // ═════════════════════════════════════════════════════════════════════════
    let toolbar_y = y0;
    c.fill_rect(0, toolbar_y, fw, TOOLBAR_H, ExpPal::TOOLBAR_BG);
    c.hline(0, toolbar_y + TOOLBAR_H - 1, fw, ExpPal::BORDER);

    let tabs: &[(&str, ExplorerView)] = &[
        ("  Archivos  ", ExplorerView::Files),
        ("  Marcadores", ExplorerView::Bookmarks),
        ("  Recientes ", ExplorerView::Recent),
    ];
    let mut tx = 0usize;
    for &(label, view) in tabs.iter() {
        let is_act = exp.view == view;
        let tw = label.len() * cw + 2;
        let bg = if is_act { ExpPal::BG } else { ExpPal::TOOLBAR_BG };
        c.fill_rect(tx, toolbar_y, tw, TOOLBAR_H, bg);
        if is_act {
            c.fill_rect(tx, toolbar_y + TOOLBAR_H - 2, tw, 2, ExpPal::ACCENT);
        }
        c.vline(tx + tw - 1, toolbar_y, TOOLBAR_H, ExpPal::BORDER);
        let fg = if is_act { ExpPal::TEXT_SEL } else { ExpPal::TEXT_DIM };
        c.write_at(label, tx + 4, toolbar_y + (TOOLBAR_H - ch) / 2, fg);
        tx += tw;
    }

    // Botón [?] ayuda — derecha
    let hx = fw.saturating_sub(cw * 2 + 14);
    let hy = toolbar_y + (TOOLBAR_H - 16) / 2;
    c.fill_rect(hx, hy, cw * 2 + 10, 16, ExpPal::TOOLBAR_BG);
    c.draw_rect(hx, hy, cw * 2 + 10, 16, 1, ExpPal::BORDER_BRIG);
    c.write_at("?", hx + 5 + cw / 2, hy + (16 - ch) / 2, ExpPal::TEXT_DIM);

    // VFS toggle [⊟/⊞] — al lado del ?
    let vx = fw.saturating_sub(cw * 2 + 14 + cw * 3 + 14 + 6);
    let vy = toolbar_y + (TOOLBAR_H - 16) / 2;
    c.fill_rect(vx, vy, cw * 3 + 10, 16, ExpPal::TOOLBAR_BG);
    c.draw_rect(vx, vy, cw * 3 + 10, 16, 1, ExpPal::BORDER_BRIG);
    let vfs_label = if exp.show_vfs { "VFS" } else { "vfs" };
    let vfg = if exp.show_vfs { ExpPal::ACCENT2 } else { ExpPal::TEXT_DIM };
    c.write_at(vfs_label, vx + 5, vy + (16 - ch) / 2, vfg);

    // ═════════════════════════════════════════════════════════════════════════
    // BREADCRUMB
    // ═════════════════════════════════════════════════════════════════════════
    let hdr_y = y0 + TOOLBAR_H;
    c.fill_rect(0, hdr_y, fw, HDR_H, ExpPal::HDR_BG);
    c.hline(0, hdr_y + HDR_H - 1, fw, ExpPal::BORDER);

    // Icono de disco pequeño
    c.write_at("[HDD]", 6, hdr_y + (HDR_H - ch) / 2, ExpPal::GOLD);

    // Path breadcrumbs
    let mut bx = 6 + 6 * cw;
    for i in 0..exp.path_depth {
        let node = &exp.path_stack[i];
        let name = node.name_str();
        let is_last = i + 1 == exp.path_depth;
        let fg = if is_last { ExpPal::TEXT } else { ExpPal::TEXT_DIM };
        c.write_at(name, bx, hdr_y + (HDR_H - ch) / 2, fg);
        bx += name.len() * cw;
        if !is_last {
            c.write_at(">", bx + 2, hdr_y + (HDR_H - ch) / 2, ExpPal::TEXT_DIM);
            bx += cw + 6;
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // CONTENIDO — según la vista activa
    // ═════════════════════════════════════════════════════════════════════════
    let content_y = hdr_y + HDR_H;
    let status_y  = bot.saturating_sub(STATUS_H);
    let preview_y = status_y.saturating_sub(PREVIEW_H);
    let list_area_h = preview_y.saturating_sub(content_y + COL_HDR_H);
    let visible = (list_area_h / ROW_H).max(1);

    match exp.view {
        ExplorerView::Files     => draw_files_view(c, lay, exp, content_y, preview_y, visible, cw, ch, fw),
        ExplorerView::Bookmarks => draw_bookmarks_view(c, lay, exp, content_y, preview_y, cw, ch, fw),
        ExplorerView::Recent    => draw_recent_view(c, lay, exp, content_y, preview_y, cw, ch, fw),
    }

    // ═════════════════════════════════════════════════════════════════════════
    // PANEL PREVIEW (común a todas las vistas)
    // ═════════════════════════════════════════════════════════════════════════
    draw_preview_panel(c, exp, preview_y, fw, cw, ch);

    // ═════════════════════════════════════════════════════════════════════════
    // STATUS BAR
    // ═════════════════════════════════════════════════════════════════════════
    let in_inp = exp.input.is_active();
    let st_bg = if in_inp && exp.input.mode == InputMode::Delete { INPUT_BG_DELETE }
        else if in_inp { INPUT_BG }
        else if exp.status_ok { ExpPal::STATUS_BG }
        else { ExpPal::STATUS_ERR };

    c.fill_rect(0, status_y, fw, STATUS_H, st_bg);
    c.hline(0, status_y, fw, Color::new(0x00, 0x55, 0xBB));

    if in_inp {
        draw_input_overlay(c, &exp.input, 8, status_y, fw, STATUS_H, cw, ch);
    } else {
        let sty = status_y + (STATUS_H - ch) / 2;
        let status = core::str::from_utf8(&exp.status[..exp.status_len]).unwrap_or("");
        c.write_at(status, 8, sty, Color::WHITE);

        let mut cb = [0u8; 24]; let mut cp = 0; let mut tmp = [0u8; 8];
        let ns = fmt_usize_local(exp.entry_count, &mut tmp);
        for b in ns.bytes() { if cp < 16 { cb[cp] = b; cp += 1; } }
        for b in b" elementos" { if cp < 24 { cb[cp] = *b; cp += 1; } }
        let cs = core::str::from_utf8(&cb[..cp]).unwrap_or("");
        c.write_at(cs, fw.saturating_sub(cs.len() * cw + 8), sty, Color::WHITE);
    }

    // ═════════════════════════════════════════════════════════════════════════
    // MENÚ CONTEXTUAL (siempre encima)
    // ═════════════════════════════════════════════════════════════════════════
    if exp.context.visible {
        draw_context_menu(c, &exp.context, fw, bot, cw, ch);
    }

    // ═════════════════════════════════════════════════════════════════════════
    // OVERLAY DE AYUDA
    // ═════════════════════════════════════════════════════════════════════════
    if exp.show_help {
        draw_help_overlay(c, lay);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Vista principal de archivos
// ─────────────────────────────────────────────────────────────────────────────

fn draw_files_view(
    c: &mut Console, _lay: &Layout, exp: &ExplorerState,
    content_y: usize, preview_y: usize, visible: usize,
    cw: usize, ch: usize, fw: usize,
) {
    let vfs_w   = if exp.show_vfs { SIDEBAR_W } else { 0 };
    let tree_x  = vfs_w;
    let tree_end = tree_x + TREE_W;
    let list_x  = tree_end + 1;
    let list_w  = fw.saturating_sub(list_x + SCR_W);
    let col_area_h = preview_y.saturating_sub(content_y);

    // ── VFS Sidebar ──────────────────────────────────────────────────────────
    if exp.show_vfs {
        c.fill_rect(0, content_y, vfs_w, col_area_h, ExpPal::SIDEBAR_BG);
        c.vline(vfs_w, content_y, col_area_h, ExpPal::BORDER_BRIG);
        // Cabecera
        c.fill_rect(0, content_y, vfs_w, COL_HDR_H, ExpPal::COL_HDR_BG);
        c.write_at("VFS", 8, content_y + (COL_HDR_H - ch) / 2, ExpPal::ACCENT2);
        c.hline(0, content_y + COL_HDR_H - 1, vfs_w, ExpPal::BORDER);

        let rows = (col_area_h.saturating_sub(COL_HDR_H)) / ROW_H;
        for (vi, entry) in VFS_TREE.iter().enumerate().take(rows) {
            let vy = content_y + COL_HDR_H + vi * ROW_H;
            let is_sel = vi == exp.vfs_sel;
            if is_sel {
                c.fill_rect(0, vy, vfs_w, ROW_H, ExpPal::ROW_SEL);
                c.fill_rect(0, vy, 2, ROW_H, ExpPal::ACCENT);
            }
            let icon_fg = if entry.is_user { Color::new(0x44, 0xCC, 0x88) } else { ExpPal::FILE_ICON };
            let lbl_fg  = if is_sel { ExpPal::TEXT_SEL } else { ExpPal::VFS_FG };
            let max_c   = (vfs_w.saturating_sub(32)) / cw;
            let lbl     = if entry.label.len() > max_c { &entry.label[..max_c] } else { entry.label };
            c.write_at(entry.icon, 4, vy + (ROW_H - ch) / 2, icon_fg);
            c.write_at(lbl, 4 + 4 * cw + 2, vy + (ROW_H - ch) / 2, lbl_fg);
        }
    }

    // ── Árbol de rutas ────────────────────────────────────────────────────────
    c.fill_rect(tree_x, content_y, TREE_W, col_area_h, Color::new(0x1A, 0x1A, 0x1A));
    c.vline(tree_end, content_y, col_area_h, ExpPal::BORDER_BRIG);
    c.fill_rect(tree_x, content_y, TREE_W, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.write_at("Ubicación", tree_x + 8, content_y + (COL_HDR_H - ch) / 2, ExpPal::TEXT_DIM);
    c.hline(tree_x, content_y + COL_HDR_H - 1, TREE_W, ExpPal::BORDER);

    let tree_rows = (col_area_h.saturating_sub(COL_HDR_H)) / ROW_H;
    let start_depth = if exp.path_depth > tree_rows { exp.path_depth - tree_rows } else { 0 };
    for i in start_depth..exp.path_depth {
        let node = &exp.path_stack[i];
        let row  = i - start_depth;
        let ty   = content_y + COL_HDR_H + row * ROW_H;
        let is_cur = i + 1 == exp.path_depth;
        let indent = i * 6;
        if is_cur {
            c.fill_rect(tree_x, ty, TREE_W, ROW_H, ExpPal::ROW_SEL);
            c.fill_rect(tree_x, ty, 2, ROW_H, ExpPal::ACCENT);
        }
        let name  = node.name_str();
        let max_c = (TREE_W.saturating_sub(indent + 16)) / cw;
        let disp  = if name.len() > max_c { &name[..max_c] } else { name };
        let fg    = if is_cur { ExpPal::TEXT_SEL } else { ExpPal::VFS_FG };
        if i > 0 { c.write_at(">", tree_x + indent + 4, ty + (ROW_H - ch) / 2, ExpPal::BORDER_BRIG); }
        c.write_at(disp, tree_x + indent + (if i > 0 { cw + 8 } else { 6 }), ty + (ROW_H - ch) / 2, fg);
    }

    // ── Lista de archivos ─────────────────────────────────────────────────────
    // Cabecera columnas
    c.fill_rect(list_x, content_y, list_w, COL_HDR_H, ExpPal::COL_HDR_BG);
    let size_col_x  = fw.saturating_sub(SCR_W + 72);
    let type_col_x  = size_col_x.saturating_sub(48);
    c.write_at("Nombre", list_x + 32, content_y + (COL_HDR_H - ch) / 2, ExpPal::TEXT_DIM);
    c.write_at("Tipo",   type_col_x,  content_y + (COL_HDR_H - ch) / 2, ExpPal::TEXT_DIM);
    c.write_at("Tamaño", size_col_x,  content_y + (COL_HDR_H - ch) / 2, ExpPal::TEXT_DIM);
    c.hline(list_x, content_y + COL_HDR_H - 1, list_w, ExpPal::BORDER);

    // Scrollbar track
    let sb_x = fw.saturating_sub(SCR_W);
    c.fill_rect(sb_x, content_y + COL_HDR_H, SCR_W, list_area_h(preview_y, content_y), ExpPal::SCR_BG);

    let scroll = compute_scroll(exp.scroll, exp.selected, visible);

    for vis in 0..visible {
        let idx = scroll + vis;
        if idx >= exp.entry_count { break; }
        let py  = content_y + COL_HDR_H + vis * ROW_H;
        let is_sel = idx == exp.selected;
        let bg  = if is_sel { ExpPal::ROW_SEL } else if vis % 2 == 0 { ExpPal::ROW_EVEN } else { ExpPal::ROW_ODD };
        c.fill_rect(list_x, py, list_w, ROW_H, bg);
        if is_sel { c.fill_rect(list_x, py, 3, ROW_H, ExpPal::ACCENT); }

        if let Some(entry) = &exp.entries[idx] {
            let name = entry.name_str();
            let kind = file_kind(name, entry.is_dir);
            let (icon_str, icon_col) = kind_icon_ascii(kind);
            let name_col = kind_fg(kind, is_sel);
            let tty = py + (ROW_H - ch) / 2;

            c.write_at(icon_str, list_x + 4, tty, icon_col);

            let max_nc = type_col_x.saturating_sub(list_x + 36) / cw;
            let ndisp  = if name.len() > max_nc && max_nc > 2 { &name[..max_nc - 1] } else { name };
            c.write_at(ndisp, list_x + 36, tty, name_col);
            if name.len() > max_nc && max_nc > 2 {
                c.write_at("~", list_x + 36 + max_nc * cw - cw, tty, ExpPal::TEXT_DIM);
            }

            let type_str = if entry.is_dir { "DIR" } else { file_ext(name) };
            c.write_at(type_str, type_col_x, tty, ExpPal::TYPE_FG);

            if !entry.is_dir {
                let mut sb = [0u8; 16];
                let ss = fmt_size_local(entry.size, &mut sb);
                c.write_at(ss, fw.saturating_sub(SCR_W + ss.len() * cw + 4), tty, ExpPal::SIZE_FG);
            } else {
                c.write_at("-", size_col_x + 8, tty, ExpPal::TEXT_DIM);
            }
        }
    }

    // Scrollbar thumb
    let la_h = list_area_h(preview_y, content_y);
    if exp.entry_count > visible && visible > 0 {
        let th_h  = (la_h * visible / exp.entry_count).max(6).min(la_h);
        let th_y  = content_y + COL_HDR_H + (scroll * la_h / exp.entry_count).min(la_h.saturating_sub(th_h));
        c.fill_rounded(sb_x + 1, th_y, SCR_W - 2, th_h, 2, ExpPal::SCR_FG);
    }

    // Mensaje vacío
    if exp.entry_count == 0 {
        c.write_at("Directorio vacío", list_x + 20, content_y + COL_HDR_H + 20, ExpPal::TEXT_DIM);
    }
}

fn list_area_h(preview_y: usize, content_y: usize) -> usize {
    preview_y.saturating_sub(content_y + COL_HDR_H)
}

// ─────────────────────────────────────────────────────────────────────────────
// Vista de marcadores
// ─────────────────────────────────────────────────────────────────────────────

fn draw_bookmarks_view(c: &mut Console, _lay: &Layout, exp: &ExplorerState, content_y: usize, preview_y: usize, cw: usize, ch: usize, fw: usize) {
    let h = preview_y.saturating_sub(content_y);
    c.fill_rect(0, content_y, fw, h, ExpPal::SIDEBAR_BG);

    // Cabecera
    c.fill_rect(0, content_y, fw, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.write_at("Marcadores", 12, content_y + (COL_HDR_H - ch) / 2, ExpPal::ACCENT2);
    c.hline(0, content_y + COL_HDR_H - 1, fw, ExpPal::BORDER);

    if exp.bookmark_count == 0 {
        c.write_at("Sin marcadores. Clic derecho en una carpeta > Agregar marcador", 16, content_y + COL_HDR_H + 20, ExpPal::TEXT_DIM);
        return;
    }

    for i in 0..exp.bookmark_count {
        let by = content_y + COL_HDR_H + i * ROW_H;
        let bg = if i % 2 == 0 { ExpPal::ROW_EVEN } else { ExpPal::ROW_ODD };
        c.fill_rect(0, by, fw, ROW_H, bg);
        c.write_at("[⭐]", 8, by + (ROW_H - ch) / 2, ExpPal::GOLD);
        let path = exp.bookmarks[i].path_str();
        c.write_at(path, 8 + 5 * cw, by + (ROW_H - ch) / 2, ExpPal::DIR_FG);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Vista de recientes
// ─────────────────────────────────────────────────────────────────────────────

fn draw_recent_view(c: &mut Console, _lay: &Layout, exp: &ExplorerState, content_y: usize, preview_y: usize, cw: usize, ch: usize, fw: usize) {
    let h = preview_y.saturating_sub(content_y);
    c.fill_rect(0, content_y, fw, h, ExpPal::SIDEBAR_BG);

    c.fill_rect(0, content_y, fw, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.write_at("Archivos recientes", 12, content_y + (COL_HDR_H - ch) / 2, ExpPal::ACCENT2);
    c.hline(0, content_y + COL_HDR_H - 1, fw, ExpPal::BORDER);

    if exp.recent_count == 0 {
        c.write_at("Sin archivos recientes. Abre un archivo para verlo aquí.", 16, content_y + COL_HDR_H + 20, ExpPal::TEXT_DIM);
        return;
    }

    for i in 0..exp.recent_count {
        let ry = content_y + COL_HDR_H + i * ROW_H;
        let bg = if i % 2 == 0 { ExpPal::ROW_EVEN } else { ExpPal::ROW_ODD };
        c.fill_rect(0, ry, fw, ROW_H, bg);
        let name = core::str::from_utf8(&exp.recent[i][..exp.recent_lens[i]]).unwrap_or("?");
        let kind = file_kind(name, false);
        let (icon, icol) = kind_icon_ascii(kind);
        c.write_at(icon, 8, ry + (ROW_H - ch) / 2, icol);
        c.write_at(name, 8 + 5 * cw, ry + (ROW_H - ch) / 2, ExpPal::FILE_FG);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Panel de preview
// ─────────────────────────────────────────────────────────────────────────────

fn draw_preview_panel(c: &mut Console, exp: &ExplorerState, preview_y: usize, fw: usize, cw: usize, ch: usize) {
    c.hline(0, preview_y, fw, ExpPal::BORDER_BRIG);
    c.fill_rect(0, preview_y + 1, fw, PREVIEW_H - 1, ExpPal::PREVIEW_BG);

    // Cabecera preview
    c.fill_rect(0, preview_y + 1, fw, COL_HDR_H, ExpPal::COL_HDR_BG);
    c.hline(0, preview_y + COL_HDR_H, fw, ExpPal::BORDER);
    c.write_at("Vista previa", 8, preview_y + 1 + (COL_HDR_H - ch) / 2, Color::new(0x00, 0xCC, 0x88));

    if exp.preview_len > 0 {
        let prev_name = core::str::from_utf8(&exp.preview_name[..exp.preview_nlen]).unwrap_or("");
        c.write_at("—", 8 + 14 * cw - cw, preview_y + 1 + (COL_HDR_H - ch) / 2, ExpPal::TEXT_DIM);
        c.write_at(prev_name, 8 + 14 * cw, preview_y + 1 + (COL_HDR_H - ch) / 2, ExpPal::TEXT);

        let data = &exp.preview[..exp.preview_len];
        let mut ls = 0usize; let mut ln = 0usize;
        let ty0 = preview_y + 1 + COL_HDR_H + 3;
        for i in 0..=data.len() {
            if (i == data.len() || data[i] == b'\n') && ln < PREVIEW_LINES {
                let bytes = &data[ls..i];
                let mc    = fw.saturating_sub(16) / cw;
                let disp  = &bytes[..bytes.len().min(mc)];
                if let Ok(s) = core::str::from_utf8(disp) { c.write_at(s, 8, ty0 + ln * (ch + 2), ExpPal::PREVIEW_FG); }
                ln += 1; ls = i + 1;
            }
        }
    } else if exp.entry_count > 0 {
        c.write_at("Selecciona un archivo de texto para previsualizar", 8, preview_y + 1 + COL_HDR_H + 6, ExpPal::TEXT_DIM);
    } else {
        c.write_at("Directorio vacío", 8, preview_y + 1 + COL_HDR_H + 6, ExpPal::TEXT_DIM);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_context_menu
// ─────────────────────────────────────────────────────────────────────────────

fn draw_context_menu(c: &mut Console, ctx: &ContextMenu, fw: usize, bot: usize, cw: usize, ch: usize) {
    let mw = ctx.width(cw);
    let mh = ctx.height();

    // Ajustar posición para no salir de pantalla
    let mx = if ctx.x + mw + 4 > fw { fw.saturating_sub(mw + 4) } else { ctx.x };
    let my = if ctx.y + mh + 4 > bot { bot.saturating_sub(mh + 4) } else { ctx.y };

    // Sombra
    c.fill_rect(mx + 3, my + 3, mw, mh, Color::new(0x00, 0x00, 0x00));
    // Fondo
    c.fill_rect(mx, my, mw, mh, ExpPal::CONTEXT_BG);
    // Borde
    c.draw_rect(mx, my, mw, mh, 1, ExpPal::CONTEXT_BOR);
    // Línea de acento superior
    c.fill_rect(mx, my, mw, 2, ExpPal::ACCENT);

    for (ii, item) in ctx.items[..ctx.item_count].iter().enumerate() {
        let iy  = my + 2 + ii * CONTEXT_ITEM_H;
        let tty = iy + (CONTEXT_ITEM_H - ch) / 2;
        if item.action == ContextAction::Separator {
            c.hline(mx + 6, iy + CONTEXT_ITEM_H / 2, mw - 12, ExpPal::CONTEXT_SEP);
        } else {
            let is_hov = ii == ctx.hovered;
            if is_hov { c.fill_rect(mx + 1, iy, mw - 2, CONTEXT_ITEM_H, ExpPal::CONTEXT_HOV); }
            let fg = if is_hov { Color::WHITE } else { ExpPal::TEXT };
            // Pequeño punto de color de acción
            let dot_col = match item.action {
                ContextAction::Delete | ContextAction::Rename => Color::new(0xCC, 0x44, 0x44),
                ContextAction::NewFolder | ContextAction::NewFile => Color::new(0x44, 0xCC, 0x88),
                ContextAction::OpenWithIde => ExpPal::ACCENT2,
                _ => ExpPal::TEXT_DIM,
            };
            c.fill_rounded(mx + 6, tty + ch / 2 - 2, 4, 4, 2, dot_col);
            c.write_at(item.label, mx + 14, tty, fg);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_help_overlay
// ─────────────────────────────────────────────────────────────────────────────

fn draw_help_overlay(c: &mut Console, lay: &Layout) {
    const OW: usize = 400;
    const OH: usize = 260;
    let fw = lay.fw;
    let cw = lay.font_w;
    let ch = lay.font_h;

    // Atenuar fondo
    c.fill_rect_alpha(0, lay.content_y, fw, lay.bottom_y.saturating_sub(lay.content_y), Color::new(0,0,0), 160);

    let ox = (fw.saturating_sub(OW)) / 2;
    let oy = (lay.bottom_y.saturating_sub(OH)) / 2;

    c.fill_rect(ox, oy, OW, OH, ExpPal::OVERLAY_BG);
    c.draw_rect(ox, oy, OW, OH, 1, ExpPal::CONTEXT_BOR);
    c.fill_rect(ox, oy, OW, 24, ExpPal::ACCENT);
    c.write_at("Atajos — Explorador de Archivos", ox + 10, oy + (24 - ch) / 2, Color::WHITE);
    c.write_at("[Cualquier tecla]", ox + OW - 18 * cw - 6, oy + (24 - ch) / 2, Color::new(0xCC, 0xFF, 0xFF));

    let entries: &[(&str, &str)] = &[
        ("Flechas",  "Navegar lista"),
        ("Enter",    "Abrir/entrar"),
        ("Backspace","Subir directorio"),
        ("N",        "Nueva carpeta"),
        ("F",        "Nuevo archivo"),
        ("D / Supr", "Eliminar"),
        ("Tab",      "Cambiar vista"),
        ("──────────", ""),
        ("Clic der", "Menú contextual"),
        ("F1 / [?]", "Esta ayuda"),
        ("F5",       "Actualizar"),
        ("──────────", ""),
        ("Vistas",   "Archivos / Marcadores / Recientes"),
    ];

    let row_h = ch + 5;
    for (i, (key, desc)) in entries.iter().enumerate() {
        let ex = ox + 16;
        let ey = oy + 32 + i * row_h;
        if key.starts_with('─') {
            c.hline(ex, ey + ch / 2, OW - 32, Color::new(0x3A, 0x3A, 0x3A));
        } else {
            c.write_at(key, ex, ey, Color::new(0x9C, 0xCB, 0xFF));
            let kw = 12 * cw;
            c.write_at(desc, ex + kw, ey, ExpPal::TEXT);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn compute_scroll(prev: usize, sel: usize, vis: usize) -> usize {
    if sel < prev { sel }
    else if vis > 0 && sel >= prev + vis { sel + 1 - vis }
    else { prev }
}

fn file_ext(name: &str) -> &'static str {
    if name.ends_with(".rs") { "RS" }
    else if name.ends_with(".c") { "C" }
    else if name.ends_with(".h") { "H" }
    else if name.ends_with(".asm") || name.ends_with(".s") { "ASM" }
    else if name.ends_with(".txt") { "TXT" }
    else if name.ends_with(".md") { "MD" }
    else if name.ends_with(".bin") { "BIN" }
    else if name.ends_with(".elf") { "ELF" }
    else if name.ends_with(".bmp") { "BMP" }
    else if name.ends_with(".toml") { "TOML" }
    else { "---" }
}

fn fmt_size_local(bytes: u32, buf: &mut [u8; 16]) -> &str {
    let mut p = 0usize; let mut tmp = [0u8; 8];
    if bytes < 1024 {
        let s = fmt_usize_local(bytes as usize, &mut tmp);
        for b in s.bytes() { if p < 10 { buf[p] = b; p += 1; } }
        for b in b" B" { if p < 14 { buf[p] = *b; p += 1; } }
    } else if bytes < 1024 * 1024 {
        let s = fmt_usize_local((bytes / 1024) as usize, &mut tmp);
        for b in s.bytes() { if p < 10 { buf[p] = b; p += 1; } }
        for b in b" KB" { if p < 14 { buf[p] = *b; p += 1; } }
    } else {
        let s = fmt_usize_local((bytes / (1024 * 1024)) as usize, &mut tmp);
        for b in s.bytes() { if p < 10 { buf[p] = b; p += 1; } }
        for b in b" MB" { if p < 14 { buf[p] = *b; p += 1; } }
    }
    core::str::from_utf8(&buf[..p]).unwrap_or("?")
}

fn fmt_usize_local(mut n: usize, buf: &mut [u8]) -> &str {
    let mut i = buf.len(); if i == 0 { return ""; }
    if n == 0 { buf[i - 1] = b'0'; return core::str::from_utf8(&buf[i - 1..]).unwrap_or("0"); }
    while n > 0 && i > 0 { i -= 1; buf[i] = b'0' + (n % 10) as u8; n /= 10; }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}