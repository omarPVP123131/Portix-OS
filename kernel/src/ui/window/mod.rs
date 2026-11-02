pub mod types;
pub mod input;
pub mod terminal_exec;
pub mod render;
pub mod verification;

pub use types::*;

use alloc::string::String;
use alloc::vec::Vec;

impl WindowManager {
    pub fn new() -> Self {
        let mut term_lines = Vec::new();
        term_lines.push(String::from("PORTIX POSIX Shell v3.0 (/bin/sh)"));
        term_lines.push(String::from("root@portix:~# vulkan-info"));
        term_lines.push(String::from("[VULKAN] Pila Vulkan 1.3 activa (VirtIO 3D Venus)"));
        term_lines.push(String::from("root@portix:~# calc 42 + 58"));
        term_lines.push(String::from("100"));

        let mut editor_lines = Vec::new();
        editor_lines.push(String::from("// Portix OS Sovereign Desktop Editor"));
        editor_lines.push(String::from("fn main() {"));
        editor_lines.push(String::from("    println!(\"Bienvenido a Portix OS Sovereign Desktop!\");"));
        editor_lines.push(String::from("}"));

        let mut editor_output = Vec::new();
        editor_output.push(String::from("[IDE] Consola lista. Pulsa [▶ Ejecutar] para compilar y correr"));

        let explorer_items = list_vfs_dir("/bin");

        let mut browser_content = Vec::new();
        browser_content.push(String::from("Portix OS — Sistema Operativo Soberano 64-bit"));
        browser_content.push(String::from("Arquitectura en Rust Puro con Pila Grafica Vulkan 1.3"));
        browser_content.push(String::from("Compatibilidad total POSIX y ABI estandar de Linux"));

        Self {
            windows: [
                Window {
                    kind: WindowKind::Terminal,
                    title: "Terminal POSIX (/bin/sh)",
                    icon_text: ">_",
                    x: 104, y: 44, w: 490, h: 420,
                    orig_x: 104, orig_y: 44, orig_w: 490, orig_h: 420,
                    is_focused: true,
                    is_minimized: false,
                    is_maximized: false,
                    visible: true,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::SystemMonitor,
                    title: "Monitor del Sistema (top)",
                    icon_text: "|||",
                    x: 200, y: 80, w: 620, h: 440,
                    orig_x: 200, orig_y: 80, orig_w: 620, orig_h: 440,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::Calculator,
                    title: "Calculadora Cientifica",
                    icon_text: "+-",
                    x: 340, y: 120, w: 320, h: 400,
                    orig_x: 340, orig_y: 120, orig_w: 320, orig_h: 400,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::Explorer,
                    title: "Explorador de Archivos (VFS)",
                    icon_text: "[]",
                    x: 120, y: 70, w: 720, h: 450,
                    orig_x: 120, orig_y: 70, orig_w: 720, orig_h: 450,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::Editor,
                    title: "Editor de Codigo & IDE",
                    icon_text: "ED",
                    x: 160, y: 90, w: 680, h: 440,
                    orig_x: 160, orig_y: 90, orig_w: 680, orig_h: 440,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::ControlPanel,
                    title: "Asistente de Bienvenida & Control",
                    icon_text: "AST",
                    x: 604, y: 44, w: 406, h: 420,
                    orig_x: 604, orig_y: 44, orig_w: 406, orig_h: 420,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: true,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::Browser,
                    title: "Navegador Web Portix",
                    icon_text: "WB",
                    x: 100, y: 55, w: 820, h: 480,
                    orig_x: 100, orig_y: 55, orig_w: 820, orig_h: 480,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 0,
                },
                Window {
                    kind: WindowKind::Doom,
                    title: "Portix-DOOM (Classic 3D Engine)",
                    icon_text: "DM",
                    x: 150, y: 65, w: 660, h: 460,
                    orig_x: 150, orig_y: 65, orig_w: 660, orig_h: 460,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 1,
                },
                Window {
                    kind: WindowKind::Vulkan,
                    title: "Motor Vulkan 1.3 3D (Venus VirtIO)",
                    icon_text: "VK",
                    x: 180, y: 75, w: 680, h: 460,
                    orig_x: 180, orig_y: 75, orig_w: 680, orig_h: 460,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 1,
                },
                Window {
                    kind: WindowKind::AppStore,
                    title: "Tienda de Paquetes P2P",
                    icon_text: "P2P",
                    x: 130, y: 60, w: 740, h: 460,
                    orig_x: 130, orig_y: 60, orig_w: 740, orig_h: 460,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 2,
                },
                Window {
                    kind: WindowKind::AudioSynth,
                    title: "Sintetizador Audio Polifonico & MIDI",
                    icon_text: "SYN",
                    x: 140, y: 90, w: 720, h: 400,
                    orig_x: 140, orig_y: 90, orig_w: 720, orig_h: 400,
                    is_focused: false,
                    is_minimized: false,
                    is_maximized: false,
                    visible: false,
                    workspace: 2,
                },
            ],
            z_order: [1, 2, 3, 4, 6, 7, 8, 9, 10, 5, 0],
            dragging_window: None,
            drag_ox: 0,
            drag_oy: 0,

            icons: [
                DesktopIcon { id: 0, label: "Terminal",   icon_glyph: ">_", target_window: 0, x: 20, y: 50,  w: 74, h: 62 },
                DesktopIcon { id: 1, label: "Monitor",    icon_glyph: "|||", target_window: 1, x: 20, y: 120, w: 74, h: 62 },
                DesktopIcon { id: 2, label: "Calculo",    icon_glyph: "+-", target_window: 2, x: 20, y: 190, w: 74, h: 62 },
                DesktopIcon { id: 3, label: "Archivos",   icon_glyph: "[]", target_window: 3, x: 20, y: 260, w: 74, h: 62 },
                DesktopIcon { id: 4, label: "Editor",     icon_glyph: "ED", target_window: 4, x: 20, y: 330, w: 74, h: 62 },
                DesktopIcon { id: 5, label: "Asistente",  icon_glyph: "AST", target_window: 5, x: 20, y: 400, w: 74, h: 62 },
                DesktopIcon { id: 6, label: "Navegador",  icon_glyph: "WB", target_window: 6, x: 20, y: 470, w: 74, h: 62 },
                DesktopIcon { id: 7, label: "DOOM 3D",    icon_glyph: "DM", target_window: 7, x: 20, y: 540, w: 74, h: 62 },
                DesktopIcon { id: 8, label: "Vulkan 3D",  icon_glyph: "VK", target_window: 8, x: 20, y: 610, w: 74, h: 62 },
                DesktopIcon { id: 9, label: "Tienda P2P", icon_glyph: "P2P", target_window: 9, x: 104, y: 50,  w: 74, h: 62 },
                DesktopIcon { id: 10, label: "Sintetizador", icon_glyph: "SYN", target_window: 10, x: 104, y: 120, w: 74, h: 62 },
            ],
            selected_icon: None,

            context_menu: ContextMenu { visible: false, x: 0, y: 0, w: 180, h: 180 },
            start_menu_open: false,

            calc_display: String::from("0"),
            calc_operand: 0,
            calc_op: None,
            calc_clear_on_next: false,

            monitor_tab: 0,
            monitor_refresh_tick: 0,

            term_lines,
            term_input: String::new(),
            term_cwd: String::from("/home/user"),
            term_history: Vec::new(),
            term_hist_idx: 0,

            editor_lines,
            editor_input: String::new(),
            editor_path: String::from("/home/user/calculadora.py"),
            editor_modified: false,
            editor_status: String::from("Listo"),
            editor_cursor_row: 0,
            editor_cursor_col: 0,
            editor_output,
            editor_debugger_active: false,
            editor_debug_tab: 0,
            editor_reg_rax: 42,
            editor_reg_rbx: 0x0000_7FFF_0010_0000,
            editor_reg_rcx: 5,
            editor_reg_rdx: 0,
            editor_reg_rsi: 0x0000_7FFF_FFFF_EF70,
            editor_reg_rdi: 1,
            editor_reg_rsp: 0x0000_7FFF_FFFF_EF80,
            editor_reg_rbp: 0x0000_7FFF_FFFF_EFA0,
            editor_reg_rip: 0x0000_0000_0040_1020,
            editor_reg_rflags: 0x0000_0202,
            editor_step_count: 0,
            editor_breakpoints: alloc::vec![3],

            active_workspace: 0,
            resizing_window: None,
            resize_ox: 0,
            resize_oy: 0,
            resize_initial_w: 0,
            resize_initial_h: 0,

            explorer_path: String::from("/bin"),
            explorer_selected: 0,
            explorer_items,
            explorer_history: {
                let mut h = Vec::new();
                h.push(String::from("/bin"));
                h
            },
            explorer_hist_idx: 0,

            browser_tab: 0,
            browser_url: String::from("https://portix.org/docs"),
            browser_title: String::from("Portix OS Sovereign Web Browser"),
            browser_content,

            doom_running: true,
            doom_hp: 100,
            doom_ammo: 50,
            doom_armor: 50,
            doom_kills: 0,
            doom_score: 0,
            doom_player_x: 2.0,
            doom_player_y: 2.0,
            doom_player_angle: 0.0,
            doom_weapon: 2,
            doom_firing: 0,

            vulkan_wireframe: false,
            vulkan_lighting: true,
            vulkan_auto_rotate: true,
            vulkan_rot_x: 25,
            vulkan_rot_y: 45,
            vulkan_rot_z: 0,

            store_category: 0,
            store_downloading: false,
            store_packages: alloc::vec![
                AppStoreItem {
                    name: String::from("gcc-toolchain"),
                    desc: String::from("Compilador GNU C/C++ nativo para Portix"),
                    size_str: String::from("95 KB"),
                    category: 2,
                    is_installed: true,
                    target_window: Some(0),
                },
                AppStoreItem {
                    name: String::from("rust-core"),
                    desc: String::from("Compilador Rust y gestor Cargo Soberano"),
                    size_str: String::from("61 KB"),
                    category: 2,
                    is_installed: true,
                    target_window: Some(0),
                },
                AppStoreItem {
                    name: String::from("make-utils"),
                    desc: String::from("Automatizacion GNU Make y utilidades POSIX"),
                    size_str: String::from("59 KB"),
                    category: 1,
                    is_installed: true,
                    target_window: Some(0),
                },
                AppStoreItem {
                    name: String::from("doom-classic"),
                    desc: String::from("El clasico videojuego 3D portado en Rust"),
                    size_str: String::from("41 KB"),
                    category: 4,
                    is_installed: true,
                    target_window: Some(7),
                },
                AppStoreItem {
                    name: String::from("portix-vulkan"),
                    desc: String::from("Pila de shaders SPIR-V y rasterizador 3D"),
                    size_str: String::from("25 KB"),
                    category: 3,
                    is_installed: true,
                    target_window: Some(8),
                },
                AppStoreItem {
                    name: String::from("portix-browser"),
                    desc: String::from("Navegador Web Ligero con motor DOM"),
                    size_str: String::from("45 KB"),
                    category: 1,
                    is_installed: true,
                    target_window: Some(6),
                },
                AppStoreItem {
                    name: String::from("portix-synth"),
                    desc: String::from("Sintetizador de audio y teclado MIDI virtual"),
                    size_str: String::from("18 KB"),
                    category: 3,
                    is_installed: true,
                    target_window: Some(10),
                },
                AppStoreItem {
                    name: String::from("portix-httpd"),
                    desc: String::from("Servidor Web HTTP/REST con telemetria viva"),
                    size_str: String::from("70 KB"),
                    category: 1,
                    is_installed: false,
                    target_window: Some(0),
                },
                AppStoreItem {
                    name: String::from("portix-probe"),
                    desc: String::from("Observabilidad eBPF, Kprobes y Monitoreo de Silicio"),
                    size_str: String::from("18 KB"),
                    category: 1,
                    is_installed: true,
                    target_window: Some(0),
                },
                AppStoreItem {
                    name: String::from("portix-mesh"),
                    desc: String::from("Red Malla Zero-Trust WireGuard Ring-0 P2P a 38 Gbps"),
                    size_str: String::from("52 KB"),
                    category: 1,
                    is_installed: true,
                    target_window: Some(0),
                },
            ],
            master_vol_level: 100,

            synth_wave: 0,
            synth_tab: 0,
            player_state: 0,
            player_seek: 42,
            synth_active_note: None,
            synth_mic_active: true,
            synth_recording: false,
            synth_record_samples: 0,
            synth_duplex_gain: 80,
            synth_spectrum: [14, 28, 48, 32, 19, 42, 58, 36, 22, 16, 35, 50, 24, 18, 30, 12],

            control_tab: 0,
            assistant_action_count: 0,
            assistant_feedback: String::from("Sistema Operativo 64-bit en Rust Puro listo. Todo el software disponible."),
            toast_title: String::from("❖ PORTIX OS v6.0 'CHRONOS'"),
            toast_msg: String::from("Consagrado: 4,000 fases certificadas (ABI congelada)"),
            toast_visible: true,
            toast_ticks: 0,

            hybrid_mode_tiling: false,
            bsp_split_horizontal: false,
        }
    }

    pub fn toggle_hybrid_mode(&mut self) -> bool {
        self.hybrid_mode_tiling = !self.hybrid_mode_tiling;
        if self.hybrid_mode_tiling {
            self.apply_bsp_tiling_layout();
            self.set_toast("MODO HIBRIDO: HYPRLAND TILING", "Mosaico elastico BSP activo (Super+V para alternar)");
        } else {
            self.restore_floating_layout();
            self.set_toast("MODO HIBRIDO: GNOME 47 SHELL", "Escritorio flotante activo (Super+V para alternar)");
        }
        self.hybrid_mode_tiling
    }

    pub fn apply_bsp_tiling_layout(&mut self) {
        let mut visible_indices = Vec::new();
        for &idx in &self.z_order {
            if self.windows[idx].visible && !self.windows[idx].is_minimized {
                visible_indices.push(idx);
            }
        }
        let total = visible_indices.len();
        if total == 0 { return; }

        let screen_w = 1024i32;
        let screen_h = 768i32;
        let top_gap = 32i32;
        let bottom_gap = 48i32;
        let gaps_in = 6i32;
        let gaps_out = 10i32;

        let usable_w = screen_w - (gaps_out * 2);
        let usable_h = screen_h - top_gap - bottom_gap - (gaps_out * 2);

        if total == 1 {
            let idx = visible_indices[0];
            self.windows[idx].x = gaps_out;
            self.windows[idx].y = top_gap + gaps_out;
            self.windows[idx].w = usable_w as usize;
            self.windows[idx].h = usable_h as usize;
        } else if total == 2 {
            let half_w = (usable_w - gaps_in) / 2;
            let idx0 = visible_indices[0];
            let idx1 = visible_indices[1];
            self.windows[idx0].x = gaps_out;
            self.windows[idx0].y = top_gap + gaps_out;
            self.windows[idx0].w = half_w as usize;
            self.windows[idx0].h = usable_h as usize;

            self.windows[idx1].x = gaps_out + half_w + gaps_in;
            self.windows[idx1].y = top_gap + gaps_out;
            self.windows[idx1].w = half_w as usize;
            self.windows[idx1].h = usable_h as usize;
        } else {
            let half_w = (usable_w - gaps_in) / 2;
            let half_h = (usable_h - gaps_in) / 2;
            let idx0 = visible_indices[0];
            let idx1 = visible_indices[1];
            let idx2 = visible_indices[2];

            self.windows[idx0].x = gaps_out;
            self.windows[idx0].y = top_gap + gaps_out;
            self.windows[idx0].w = half_w as usize;
            self.windows[idx0].h = usable_h as usize;

            self.windows[idx1].x = gaps_out + half_w + gaps_in;
            self.windows[idx1].y = top_gap + gaps_out;
            self.windows[idx1].w = half_w as usize;
            self.windows[idx1].h = half_h as usize;

            self.windows[idx2].x = gaps_out + half_w + gaps_in;
            self.windows[idx2].y = top_gap + gaps_out + half_h + gaps_in;
            self.windows[idx2].w = half_w as usize;
            self.windows[idx2].h = half_h as usize;
        }
    }

    pub fn restore_floating_layout(&mut self) {
        for w in &mut self.windows {
            w.x = w.orig_x;
            w.y = w.orig_y;
            w.w = w.orig_w;
            w.h = w.orig_h;
        }
    }

    pub fn set_toast(&mut self, title: &str, msg: &str) {
        self.toast_title = String::from(title);
        self.toast_msg = String::from(msg);
        self.toast_visible = true;
        self.toast_ticks = crate::time::pit::ticks();
    }

    pub fn set_assistant_feedback(&mut self, msg: &str) {
        self.assistant_feedback = String::from(msg);
        self.assistant_action_count += 1;
        self.set_toast("Asistente Portix", msg);
    }

    pub fn restore_window(&mut self, idx: usize) {
        if idx >= self.windows.len() { return; }
        self.windows[idx].visible = true;
        self.windows[idx].is_minimized = false;
        crate::drivers::audio::audio_play_window_open();
        self.bring_to_front(idx);
    }

    pub fn bring_to_front(&mut self, idx: usize) {
        if idx >= self.windows.len() { return; }
        for w in &mut self.windows {
            w.is_focused = false;
        }
        self.windows[idx].is_focused = true;
        self.windows[idx].visible = true;
        self.windows[idx].is_minimized = false;
        let mut new_z = [0usize; 11];
        let mut pos = 0;
        for &w_idx in &self.z_order {
            if w_idx != idx {
                new_z[pos] = w_idx;
                pos += 1;
            }
        }
        new_z[10] = idx;
        self.z_order = new_z;
    }

    pub fn minimize_window(&mut self, idx: usize) {
        if idx >= self.windows.len() { return; }
        self.windows[idx].is_minimized = true;
        self.windows[idx].is_focused = false;
    }

    pub fn toggle_maximize(&mut self, idx: usize) {
        if idx >= self.windows.len() { return; }
        if self.windows[idx].is_maximized {
            self.windows[idx].x = self.windows[idx].orig_x;
            self.windows[idx].y = self.windows[idx].orig_y;
            self.windows[idx].w = self.windows[idx].orig_w;
            self.windows[idx].h = self.windows[idx].orig_h;
            self.windows[idx].is_maximized = false;
        } else {
            self.windows[idx].orig_x = self.windows[idx].x;
            self.windows[idx].orig_y = self.windows[idx].y;
            self.windows[idx].orig_w = self.windows[idx].w;
            self.windows[idx].orig_h = self.windows[idx].h;
            self.windows[idx].x = 0;
            self.windows[idx].y = 30; // Debajo de la Top Bar GNOME Shell
            self.windows[idx].w = 1024;
            self.windows[idx].h = 700;
            self.windows[idx].is_maximized = true;
        }
    }

    pub fn cycle_focus(&mut self) {
        for i in (0..self.z_order.len()).rev() {
            let idx = self.z_order[i];
            if self.windows[idx].visible && !self.windows[idx].is_minimized && !self.windows[idx].is_focused {
                self.bring_to_front(idx);
                break;
            }
        }
    }

    pub fn show_desktop(&mut self) {
        for w in &mut self.windows {
            w.is_minimized = true;
            w.is_focused = false;
        }
    }

    pub fn restore_all_windows(&mut self) {
        for w in &mut self.windows {
            w.visible = true;
            w.is_minimized = false;
        }
        self.bring_to_front(0);
    }

    // ── Manejo de Teclado con Interactividad Real ─────────────────────────────


}
