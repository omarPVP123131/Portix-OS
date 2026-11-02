// kernel/src/ui/window.rs — Modern Multitasking Real Desktop & Window Compositor
// Designed to provide a genuine, functional desktop experience like GNOME 45 / Wayland / Windows:
// - Top Bar GNOME Shell: Activities button, Clock/Calendar pill, Quick Settings (Network, Audio, Power)
// - Dash-to-Dock: Bottom floating dock with 11 pinned application icons and active window dots
// - Real VFS Two-Pane File Explorer: Dynamic directory scanning on FAT32/RAMFS, breadcrumbs, folder traversal,
//   document loading into editor, and binary launching
// - Real Persistent Code Editor: Real file loading from disk, keyboard editing, and physical saving to FAT32/VFS
// - Real Terminal: POSIX shell executing kernel commands and Ring-3 ELF binaries with live stdout output
// - Real System Monitor: Telemetry querying live processes from PROCESS_TABLE, Buddy Allocator memory, and e1000
// - Interactive DOOM 3D & Vulkan 1.3 Engines: Real-time keyboard/mouse rotation, movement, firing, and shading

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::graphics::driver::framebuffer::{Color, Console};

// ── Helpers de Dibujo Seguro ──────────────────────────────────────────────────

#[inline]
pub(crate) fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b)
}

#[inline]
pub(crate) fn draw_box(c: &mut Console, x: i32, y: i32, w: usize, h: usize, color: Color) {
    if x >= 0 && y >= 0 {
        c.fill_rect(x as usize, y as usize, w, h, color);
    }
}

#[inline]
pub(crate) fn draw_rounded(c: &mut Console, x: i32, y: i32, w: usize, h: usize, r: usize, color: Color) {
    if x >= 0 && y >= 0 {
        c.fill_rounded(x as usize, y as usize, w, h, r, color);
    }
}

#[inline]
pub(crate) fn draw_outline(c: &mut Console, x: i32, y: i32, w: usize, h: usize, t: usize, color: Color) {
    if x >= 0 && y >= 0 {
        c.draw_rect(x as usize, y as usize, w, h, t, color);
    }
}

#[inline]
pub(crate) fn draw_text(c: &mut Console, text: &str, x: i32, y: i32, color: Color) {
    if x >= 0 && y >= 0 {
        c.write_at(text, x as usize, y as usize, color);
    }
}

#[inline]
pub(crate) fn draw_h_line(c: &mut Console, x: i32, y: i32, len: usize, color: Color) {
    if x >= 0 && y >= 0 {
        c.hline(x as usize, y as usize, len, color);
    }
}

#[inline]
pub(crate) fn draw_v_line(c: &mut Console, x: i32, y: i32, len: usize, color: Color) {
    if x >= 0 && y >= 0 {
        c.vline(x as usize, y as usize, len, color);
    }
}

pub(crate) fn format_bytes(bytes: u32) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{}.{} MB", bytes / (1024 * 1024), (bytes % (1024 * 1024)) / 100000)
    }
}

// ── Trigonometría de Punto Fijo para Renderizado 3D Interactivo ──────────────
static SIN_90: [u8; 91] = [
    0, 4, 8, 13, 17, 22, 26, 31, 35, 40, 44, 48, 53, 57, 61, 66,
    70, 74, 79, 83, 87, 91, 95, 100, 104, 108, 112, 116, 120, 124, 128, 131,
    135, 139, 143, 146, 150, 154, 157, 161, 164, 167, 171, 174, 177, 181, 184, 187,
    190, 193, 196, 198, 201, 204, 207, 209, 212, 214, 217, 219, 221, 223, 226, 228,
    230, 232, 233, 235, 237, 238, 240, 242, 243, 244, 246, 247, 248, 249, 250, 251,
    251, 252, 253, 253, 254, 254, 255, 255, 255, 255, 255
];

pub(crate) fn sin_fp(deg: i32) -> i32 {
    let d = ((deg % 360) + 360) % 360;
    if d <= 90 {
        SIN_90[d as usize] as i32
    } else if d <= 180 {
        SIN_90[(180 - d) as usize] as i32
    } else if d <= 270 {
        -(SIN_90[(d - 180) as usize] as i32)
    } else {
        -(SIN_90[(360 - d) as usize] as i32)
    }
}

pub(crate) fn cos_fp(deg: i32) -> i32 {
    sin_fp(deg + 90)
}

// ── Operaciones Reales sobre el Sistema de Archivos VFS / FAT32 ────────────────

#[derive(Clone)]
pub struct ExplorerItem {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
    pub perms: String,
    pub kind_str: String,
}

pub fn list_vfs_dir(path: &str) -> Vec<ExplorerItem> {
    let mut items = Vec::new();
    let clean = path.trim();

    // [CICLO 51 / L3] el explorador lista por el contrato SistemaDeFicheros
    // (raiz ext2, ramfs).
    if crate::drivers::storage::vfs::es_ruta_fs(clean) {
        if let Ok(lista) = crate::drivers::storage::vfs::listar_fs_tam(clean) {
            for (name, is_dir, size) in lista {
                if name == "." || name == ".." { continue; }
                let kind_str = if is_dir { String::from("DIR") } else { String::from("TXT") };
                let perms = if is_dir { String::from("drwxr-xr-x") } else { String::from("-rw-r--r--") };
                items.push(ExplorerItem { name, is_dir, size: size as u32, perms, kind_str });
            }
        }
        return items;
    }

    // [T-001] Sin raiz montada el explorador muestra el arbol vacio: antes
    // consultaba la FAT y, si no, inventaba un listado fijo de /bin.
    crate::syscall::sin_raiz(clean);

    items
}

pub fn read_vfs_file(path: &str) -> Option<String> {
    let clean = path.trim();
    if crate::drivers::storage::procfs::is_procfs_path(clean) {
        if let Some((ft, is_dir)) = crate::drivers::storage::procfs::resolve_proc_path(clean) {
            if !is_dir {
                let content = crate::drivers::storage::procfs::generate_content(ft);
                if let Ok(s) = core::str::from_utf8(&content) {
                    return Some(String::from(s));
                }
            }
        }
    }
    if let Some(bytes) = crate::elf::read_binary_file(clean) {
        let s = core::str::from_utf8(&bytes).unwrap_or("");
        Some(String::from(s))
    } else {
        None
    }
}

pub fn write_vfs_file(path: &str, data: &[u8]) -> bool {
    let clean = path.trim();
    if clean.is_empty() { return false; }

    // [CICLO 51] ramfs, ext2 (raiz) y cualquier sistema bajo el contrato.
    if crate::drivers::storage::vfs::es_ruta_fs(clean) {
        return crate::drivers::storage::vfs::escribir_fichero_fs(clean, data);
    }

    // [T-001] sin raiz montada no hay donde escribir (rescate FAT podado)
    crate::syscall::sin_raiz(clean);
    false
}

// ── Tipos y Estructuras del Escritorio ────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Terminal,       // 0: Consola POSIX /bin/sh
    SystemMonitor,  // 1: Monitor de recursos y procesos
    Calculator,     // 2: Calculadora nativa
    Explorer,       // 3: Explorador de Archivos de doble panel
    Editor,         // 4: Editor de código y notas
    ControlPanel,   // 5: Diagnóstico y hardware
    Browser,        // 6: Navegador Web Gráfico
    Doom,           // 7: Motor DOOM Clásico 3D
    Vulkan,         // 8: Pila Gráfica Vulkan 1.3
    AppStore,       // 9: Tienda & Gestor P2P
    AudioSynth,     // 10: Sintetizador de Audio & MIDI
}

#[derive(Clone)]
pub struct Window {
    pub kind: WindowKind,
    pub title: &'static str,
    pub icon_text: &'static str,
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub orig_x: i32,
    pub orig_y: i32,
    pub orig_w: usize,
    pub orig_h: usize,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub visible: bool,
    pub workspace: usize,
}

#[derive(Clone, Copy)]
pub struct DesktopIcon {
    pub id: usize,
    pub label: &'static str,
    pub icon_glyph: &'static str,
    pub target_window: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy)]
pub struct ContextMenu {
    pub visible: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Debug)]
pub struct AppStoreItem {
    pub name: String,
    pub desc: String,
    pub size_str: String,
    pub category: usize, // 0: Todos, 1: Sistema, 2: Desarrollo, 3: Gráficos, 4: Juegos
    pub is_installed: bool,
    pub target_window: Option<usize>,
}

pub struct WindowManager {
    pub windows: [Window; 11],
    pub z_order: [usize; 11],
    pub dragging_window: Option<usize>,
    pub drag_ox: i32,
    pub drag_oy: i32,

    // Iconos de escritorio
    pub icons: [DesktopIcon; 11],
    pub selected_icon: Option<usize>,

    // Menú contextual de click derecho
    pub context_menu: ContextMenu,

    // Estado del Menú de Inicio (Activities / App Drawer)
    pub start_menu_open: bool,

    // Estado de la Calculadora
    pub calc_display: String,
    pub calc_operand: i64,
    pub calc_op: Option<char>,
    pub calc_clear_on_next: bool,

    // Estado del Monitor del Sistema
    pub monitor_tab: usize, // 0: Rendimiento/CPU, 1: Memoria RAM, 2: Procesos, 3: Red
    pub monitor_refresh_tick: u64,

    // Estado de la Terminal (Real CLI Shell)
    pub term_lines: Vec<String>,
    pub term_input: String,
    pub term_cwd: String,
    pub term_history: Vec<String>,
    pub term_hist_idx: usize,

    // Estado del Editor de Código Persistente
    pub editor_lines: Vec<String>,
    pub editor_input: String,
    pub editor_path: String,
    pub editor_modified: bool,
    pub editor_status: String,
    pub editor_cursor_row: usize,
    pub editor_cursor_col: usize,
    pub editor_output: Vec<String>,

    // Estado del IDE y Depurador Visual
    pub editor_debugger_active: bool,
    pub editor_debug_tab: usize, // 0: Registros x86_64, 1: Memoria Hex, 2: Pila / Stack, 3: Consola
    pub editor_reg_rax: u64,
    pub editor_reg_rbx: u64,
    pub editor_reg_rcx: u64,
    pub editor_reg_rdx: u64,
    pub editor_reg_rsi: u64,
    pub editor_reg_rdi: u64,
    pub editor_reg_rsp: u64,
    pub editor_reg_rbp: u64,
    pub editor_reg_rip: u64,
    pub editor_reg_rflags: u64,
    pub editor_step_count: usize,
    pub editor_breakpoints: Vec<usize>,

    // Múltiples Escritorios Virtuales (Workspaces 1 a 4)
    pub active_workspace: usize,

    // Redimensionamiento Dinámico de Ventanas
    pub resizing_window: Option<usize>,
    pub resize_ox: i32,
    pub resize_oy: i32,
    pub resize_initial_w: usize,
    pub resize_initial_h: usize,

    // Estado del Explorador de Archivos Dinámico VFS
    pub explorer_path: String,
    pub explorer_selected: usize,
    pub explorer_items: Vec<ExplorerItem>,
    pub explorer_history: Vec<String>,
    pub explorer_hist_idx: usize,

    // Estado del Navegador Web Gráfico
    pub browser_tab: usize,
    pub browser_url: String,
    pub browser_title: String,
    pub browser_content: Vec<String>,

    // Estado de Doom 3D Interactivo
    pub doom_running: bool,
    pub doom_hp: u32,
    pub doom_ammo: u32,
    pub doom_armor: u32,
    pub doom_kills: u32,
    pub doom_score: u32,
    pub doom_player_x: f32,
    pub doom_player_y: f32,
    pub doom_player_angle: f32,
    pub doom_weapon: u8,
    pub doom_firing: u8,

    // Estado de Vulkan 3D Interactivo
    pub vulkan_wireframe: bool,
    pub vulkan_lighting: bool,
    pub vulkan_auto_rotate: bool,
    pub vulkan_rot_x: i32,
    pub vulkan_rot_y: i32,
    pub vulkan_rot_z: i32,

    // Estado de la Tienda de Apps P2P
    pub store_category: usize,
    pub store_downloading: bool,
    pub store_packages: Vec<AppStoreItem>,

    // Estado del Mezclador de Audio & Volumen
    pub master_vol_level: u8,

    // Estado del Sintetizador Audio & MIDI y Audio Dúplex
    pub synth_wave: usize, // 0: Seno, 1: Cuadrada, 2: Sierra
    pub synth_tab: usize,  // 0: Sintetizador MIDI, 1: Reproductor Video 8K HDR10+
    pub player_state: usize, // 0: Playing, 1: Paused
    pub player_seek: usize,  // 0..100
    pub synth_active_note: Option<u8>,
    pub synth_mic_active: bool,
    pub synth_recording: bool,
    pub synth_record_samples: usize,
    pub synth_duplex_gain: u8,
    pub synth_spectrum: [u8; 16],

    // Centro de Control y Asistente de Inicio (Segundo Uno)
    pub control_tab: usize, // 0: Asistente & Guia, 1: Arquitectura & compatibilidad Linux, 2: Benchmarks, 3: Hardware
    pub assistant_action_count: usize,
    pub assistant_feedback: String,

    // Sistema de Notificaciones Toast Interactivas (Feedback Visual)
    pub toast_title: String,
    pub toast_msg: String,
    pub toast_visible: bool,
    pub toast_ticks: u64,

    // Épica 33: Compositor Híbrido Dinámico (Tiling Hyprland + Flotante GNOME 47)
    pub hybrid_mode_tiling: bool,
    pub bsp_split_horizontal: bool,
}

