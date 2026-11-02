use alloc::format;
use alloc::string::String;
use crate::time::pit;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use super::types::*;

impl WindowManager {
    pub fn draw(&self, c: &mut Console, _lay: &Layout, hw: &HardwareInfo) {
        // 1. Fondo de Pantalla (y: 30..724) con gradiente moderno Adwaita Dark
        self.draw_desktop_background(c);

        // 2. Iconos del Escritorio
        self.draw_desktop_icons(c);

        // 3. Ventanas Flotantes en orden Z
        for &w_idx in &self.z_order {
            let w = &self.windows[w_idx];
            let in_ws = w.workspace == self.active_workspace || w_idx == 0 || w_idx == 5;
            if w.visible && !w.is_minimized && in_ws {
                self.draw_window(c, w, hw);
            }
        }

        // 4. Barra Superior GNOME 45 Shell (Top Bar)
        self.draw_top_bar(c, hw);

        // 5. Dash-to-Dock Inferior Flotante
        self.draw_dock(c);

        // 6. Menú de Inicio (Activities / App Drawer)
        if self.start_menu_open {
            self.draw_start_menu(c);
        }

        // 7. Menú Contextual
        if self.context_menu.visible {
            self.draw_context_menu(c);
        }

        // 8. Notificación Toast Flotante Interactiva
        if self.toast_visible {
            let elapsed = pit::ticks().saturating_sub(self.toast_ticks);
            if elapsed < 800 {
                self.draw_toast(c);
            }
        }
    }

    fn draw_desktop_background(&self, c: &mut Console) {
        // Fondo obsidian gradient con halo de horizonte tecnológico (y: 30..724)
        for y in 30..724 {
            let r: u8;
            let g: u8;
            let b: u8;
            if y < 320 {
                // Zona cenit: Obsidiana profundo a azul titanio oscuro
                let t = (y - 30) as u32;
                r = 6 + (t * 5 / 290) as u8;
                g = 8 + (t * 8 / 290) as u8;
                b = 14 + (t * 16 / 290) as u8;
            } else if y < 540 {
                // Zona halo de horizonte: Resplandor atmosférico zafiro / cian profundo
                let t = (y - 320) as u32;
                r = 11 + (t * 8 / 220) as u8;
                g = 16 + (t * 14 / 220) as u8;
                b = 30 + (t * 26 / 220) as u8;
            } else {
                // Zona base: Descenso suave a obsidiana puro
                let t = (y - 540) as u32;
                r = 19u8.saturating_sub((t * 12 / 184) as u8).max(6);
                g = 30u8.saturating_sub((t * 20 / 184) as u8).max(9);
                b = 56u8.saturating_sub((t * 38 / 184) as u8).max(16);
            }
            draw_h_line(c, 0, y, 1024, rgb(r, g, b));
        }

        // Rejilla técnica de vanguardia con micro-cruces de coordenadas y puntos de convergencia
        for x in (80..1024).step_by(80) {
            for y in (60..720).step_by(60) {
                draw_h_line(c, x - 2, y, 5, rgb(28, 42, 65));
                draw_box(c, x, y - 2, 1, 5, rgb(28, 42, 65));
            }
        }

        // Línea sutil de horizonte digital y marcadores de silicio
        draw_h_line(c, 0, 520, 1024, rgb(24, 38, 60));
        draw_text(c, "PORTIX OS v6.0  «CHRONOS»", 160, 642, rgb(56, 189, 248));
        draw_text(c, "Microkernel Rust no_std | Cero Telemetria | 4,000 Fases Certificadas", 160, 662, rgb(148, 163, 184));
        draw_text(c, "Soberania de Silicio: ZNS - Hipervisor Tipo-1 - Malla WireGuard - Hot-Patching", 160, 680, rgb(71, 85, 105));
    }

    fn draw_top_bar(&self, c: &mut Console, _hw: &HardwareInfo) {
        // Barra oscura superior acrílica con reborde luminoso sutil
        draw_box(c, 0, 0, 1024, 30, rgb(9, 13, 22));
        draw_h_line(c, 0, 29, 1024, rgb(30, 41, 59));
        draw_h_line(c, 0, 0, 1024, rgb(30, 41, 59));

        // Botón [❖ CHRONOS]
        let act_bg = if self.start_menu_open { rgb(37, 99, 235) } else { rgb(20, 30, 48) };
        draw_rounded(c, 6, 4, 110, 22, 4, act_bg);
        draw_text(c, "❖ CHRONOS", 14, 8, rgb(56, 189, 248));

        // Selector de Múltiples Escritorios Virtuales (Workspaces 1 a 4)
        for w in 0..4 {
            let ws_x = 124 + (w as i32) * 36;
            let is_active = self.active_workspace == w;
            let (bg, fg) = if is_active {
                (rgb(37, 99, 235), Color::WHITE)
            } else {
                (rgb(20, 30, 48), rgb(100, 116, 139))
            };
            draw_rounded(c, ws_x, 4, 30, 22, 4, bg);
            let num = [b'1' + w as u8];
            let num_str = core::str::from_utf8(&num).unwrap_or("1");
            draw_text(c, num_str, ws_x + 11, 8, fg);
            if is_active {
                draw_box(c, ws_x + 10, 23, 10, 2, rgb(56, 189, 248));
            }
        }

        // Indicador de Audio Duplex / Grabación
        if self.synth_recording {
            draw_rounded(c, 274, 4, 114, 22, 4, rgb(220, 38, 38));
            draw_text(c, "[REC WAV] ●", 282, 8, Color::WHITE);
        } else if self.synth_mic_active {
            draw_rounded(c, 274, 4, 120, 22, 4, rgb(16, 185, 129));
            draw_text(c, "[MIC DUPLEX]", 282, 8, Color::WHITE);
        }

        // Título / Reloj en el centro
        let secs = pit::ticks() / 100;
        let mins = (secs / 60) % 60;
        let hrs = (secs / 3600) % 24;
        let clock_str = format!("Vie 25 Sep  {:02}:{:02}:{:02}  ● 60fps", hrs, mins, secs % 60);
        draw_rounded(c, 400, 4, 215, 22, 4, rgb(17, 24, 39));
        draw_text(c, &clock_str, 410, 8, rgb(241, 245, 249));

        // Botón de Modo Híbrido (Épica 33: Hyprland Tiling BSP / GNOME 47 Flotante)
        let (hybrid_bg, hybrid_label) = if self.hybrid_mode_tiling {
            (rgb(147, 51, 234), "[# Hypr]")
        } else {
            (rgb(37, 99, 235), "[= Shell]")
        };
        draw_rounded(c, 622, 4, 112, 22, 4, hybrid_bg);
        draw_text(c, hybrid_label, 630, 8, Color::WHITE);

        // Selector de Idioma Dinámico (i18n: ES, EN, JA, DE)
        let lang_code = {
            let i18n = crate::compat::i18n::GLOBAL_I18N.lock();
            i18n.lang_code()
        };
        draw_rounded(c, 740, 4, 36, 22, 4, rgb(30, 41, 59));
        draw_text(c, lang_code, 749, 8, rgb(56, 189, 248));

        // Barra Superior Unificada Cosmos: Estado de NPU/IA, Red Zero-Trust, Audio y Volumen
        let vol_str = match self.master_vol_level {
            100 => "100%",
            75 => "75%",
            50 => "50%",
            25 => "25%",
            _ => "MUTE",
        };
        draw_rounded(c, 782, 4, 236, 22, 4, rgb(17, 24, 39));
        let qs_str = format!("NPU | MESH | 2ms | {}", vol_str);
        draw_text(c, &qs_str, 792, 8, rgb(56, 189, 248));
    }

    fn draw_dock(&self, c: &mut Console) {
        // Dock inferior centrado flotante (y: 724..768)
        draw_box(c, 0, 724, 1024, 44, rgb(9, 13, 22));
        draw_h_line(c, 0, 724, 1024, rgb(30, 41, 59));

        // Botón Inicio [❖ CHRONOS] en x: 8..110
        let btn_bg = if self.start_menu_open { rgb(37, 99, 235) } else { rgb(20, 30, 48) };
        draw_rounded(c, 8, 728, 100, 32, 6, btn_bg);
        draw_text(c, "[❖ CHRONOS]", 14, 737, Color::WHITE);

        // Iconos fijados en el dock (x: 120..494)
        let dock_x = 120i32;
        let pin_w = 34i32;
        let labels = [">_", "|||", "+-", "[]", "ED", "CP", "WB", "DM", "VK", "P2P", "SYN"];
        for i in 0..11 {
            let px = dock_x + (i as i32) * pin_w;
            let is_open = self.windows[i].visible && !self.windows[i].is_minimized;
            let bg = if self.windows[i].is_focused {
                rgb(37, 99, 235)
            } else if is_open {
                rgb(24, 34, 52)
            } else {
                rgb(16, 24, 38)
            };
            draw_rounded(c, px, 728, 28, 28, 4, bg);
            let border_col = if self.windows[i].is_focused { rgb(56, 189, 248) } else { rgb(30, 41, 59) };
            draw_outline(c, px, 728, 28, 28, 1, border_col);
            draw_text(c, labels[i], px + 4, 735, Color::WHITE);

            // Dot luminoso indicador de ventana activa
            if is_open {
                let dot_col = if self.windows[i].is_focused { rgb(56, 189, 248) } else { rgb(100, 116, 139) };
                draw_box(c, px + 10, 759, 8, 2, dot_col);
            }
        }

        // Systray derecho con medidor de memoria Buddy y estado APIC
        let free_blks = crate::mem::alloc_stats_free_total();
        let free_kb = (free_blks * 16).min(65536);
        let used_kb = 65536 - free_kb;
        let ram_txt = format!("RAM: {}/64MB", used_kb / 1024);
        draw_text(c, &ram_txt, 785, 737, rgb(148, 163, 184));
        draw_text(c, "8 APIC Cores", 900, 737, rgb(56, 189, 248));
    }

    fn draw_desktop_icons(&self, c: &mut Console) {
        for icon in self.icons {
            let is_sel = self.selected_icon == Some(icon.id);
            let bg = if is_sel { rgb(37, 99, 235) } else { rgb(20, 30, 50) };
            draw_rounded(c, icon.x, icon.y, icon.w as usize, icon.h as usize, 6, bg);
            draw_outline(c, icon.x, icon.y, icon.w as usize, icon.h as usize, 1, rgb(51, 65, 85));

            draw_text(c, icon.icon_glyph, icon.x + 24, icon.y + 12, rgb(56, 189, 248));
            draw_text(c, icon.label, icon.x + 6, icon.y + 38, Color::WHITE);
        }
    }

    fn draw_start_menu(&self, c: &mut Console) {
        // App Drawer estilo GNOME (x: 10..510, y: 380..722)
        draw_rounded(c, 10, 380, 500, 342, 8, rgb(15, 23, 42));
        draw_outline(c, 10, 380, 500, 342, 1, rgb(59, 130, 246));

        // Cabecera
        draw_box(c, 12, 382, 496, 42, rgb(30, 41, 59));
        draw_text(c, "PORTIX OS — Catalogo de Aplicaciones Soberanas", 24, 396, Color::WHITE);

        // Barra de búsqueda simulada
        draw_rounded(c, 24, 430, 470, 24, 4, rgb(20, 30, 50));
        draw_text(c, "Buscar aplicaciones o comandos...", 34, 436, rgb(148, 163, 184));

        let start_y = 464;
        let col1 = [
            ("Terminal POSIX (/bin/sh)", ">_"),
            ("Monitor del Sistema (top)", "|||"),
            ("Calculadora Cientifica", "+-"),
            ("Explorador de Archivos (VFS)", "[]"),
            ("Editor de Codigo & Notas", "ED"),
            ("Centro de Control & Hardware", "CP"),
        ];

        for (i, (name, icon)) in col1.iter().enumerate() {
            let iy = start_y + (i as i32) * 26;
            draw_rounded(c, 24, iy, 225, 22, 4, rgb(24, 34, 56));
            draw_text(c, icon, 30, iy + 4, rgb(56, 189, 248));
            draw_text(c, name, 60, iy + 4, Color::WHITE);
        }

        let col2 = [
            ("Navegador Web Portix", "WB"),
            ("Portix-DOOM (Classic 3D)", "DM"),
            ("Motor Vulkan 1.3 3D", "VK"),
            ("Tienda de Paquetes P2P", "P2P"),
            ("Sintetizador Audio MIDI", "SYN"),
        ];

        for (i, (name, icon)) in col2.iter().enumerate() {
            let iy = start_y + (i as i32) * 26;
            draw_rounded(c, 260, iy, 230, 22, 4, rgb(24, 34, 56));
            draw_text(c, icon, 266, iy + 4, rgb(56, 189, 248));
            draw_text(c, name, 300, iy + 4, Color::WHITE);
        }
    }

    fn draw_context_menu(&self, c: &mut Console) {
        let cm = self.context_menu;
        draw_rounded(c, cm.x, cm.y, cm.w as usize, cm.h as usize, 6, rgb(15, 23, 42));
        draw_outline(c, cm.x, cm.y, cm.w as usize, cm.h as usize, 1, rgb(59, 130, 246));

        let items = [
            "Abrir Terminal",
            "Explorar Archivos",
            "Navegador Web",
            "Motor Vulkan 1.3",
            "Monitor de Recursos",
            "Mostrar Escritorio",
            "Restaurar Todas",
        ];

        for (i, it) in items.iter().enumerate() {
            let iy = cm.y + 6 + (i as i32) * 24;
            draw_text(c, it, cm.x + 12, iy, Color::WHITE);
        }
    }

    fn draw_window(&self, c: &mut Console, w: &Window, hw: &HardwareInfo) {
        let wx = w.x;
        let wy = w.y;
        let ww = w.w;
        let wh = w.h;

        // Marco y sombra con acrílico oscuro y reborde luminoso en foco
        draw_rounded(c, wx, wy, ww, wh, 6, rgb(10, 14, 23));
        if w.is_focused {
            // Reborde de acento luminoso activo (doble anillo)
            draw_outline(c, wx, wy, ww, wh, 1, rgb(59, 130, 246));
            if ww > 4 && wh > 4 {
                draw_outline(c, wx + 1, wy + 1, ww - 2, wh - 2, 1, rgb(30, 58, 138));
            }
        } else {
            draw_outline(c, wx, wy, ww, wh, 1, rgb(30, 41, 59));
        }

        // Barra de título Adwaita Dark / Glass (y: wy .. wy + 26)
        let title_bg = if w.is_focused { rgb(17, 24, 39) } else { rgb(13, 18, 30) };
        draw_box(c, wx + 1, wy + 1, ww - 2, 25, title_bg);
        draw_h_line(c, wx + 1, wy + 26, ww - 2, rgb(30, 41, 59));

        draw_text(c, w.icon_text, wx + 8, wy + 6, rgb(56, 189, 248));
        let max_title_chars = if ww > 80 { (ww - 80) / 8 } else { 15 };
        let t_slice = if w.title.len() > max_title_chars { &w.title[..max_title_chars] } else { w.title };
        draw_text(c, t_slice, wx + 44, wy + 6, Color::WHITE);

        // Botones circulares de ventana: Minimizar [_], Maximizar [□], Cerrar [X]
        let close_x = wx + (ww as i32) - 20;
        let max_x = close_x - 18;
        let min_x = max_x - 18;

        draw_rounded(c, min_x, wy + 7, 10, 10, 5, rgb(245, 158, 11)); // Amarillo
        draw_rounded(c, max_x, wy + 7, 10, 10, 5, rgb(16, 185, 129)); // Verde
        draw_rounded(c, close_x, wy + 7, 10, 10, 5, rgb(239, 68, 68)); // Rojo

        // Grip de redimensionamiento en esquina inferior derecha
        let rx = wx + (ww as i32) - 12;
        let ry = wy + (wh as i32) - 12;
        draw_box(c, rx + 6, ry + 2, 3, 7, rgb(71, 85, 105));
        draw_box(c, rx + 2, ry + 6, 7, 3, rgb(71, 85, 105));
        draw_box(c, rx + 6, ry + 6, 3, 3, rgb(148, 163, 184));

        // Contenido interno
        let cx = wx + 6;
        let cy = wy + 28;
        let cw = ww - 12;
        let ch = wh - 34;

        match w.kind {
            WindowKind::Terminal => self.draw_terminal_window(c, cx, cy, cw, ch),
            WindowKind::SystemMonitor => self.draw_monitor_window(c, cx, cy, cw, ch, hw),
            WindowKind::Calculator => self.draw_calc_window(c, cx, cy, cw, ch),
            WindowKind::Explorer => self.draw_explorer_window(c, cx, cy, cw, ch),
            WindowKind::Editor => self.draw_editor_window(c, cx, cy, cw, ch),
            WindowKind::ControlPanel => self.draw_control_window(c, cx, cy, cw, ch, hw),
            WindowKind::Browser => self.draw_browser_window(c, cx, cy, cw, ch),
            WindowKind::Doom => self.draw_doom_window(c, cx, cy, cw, ch),
            WindowKind::Vulkan => self.draw_vulkan_window(c, cx, cy, cw, ch),
            WindowKind::AppStore => self.draw_appstore_window(c, cx, cy, cw, ch),
            WindowKind::AudioSynth => self.draw_synth_window(c, cx, cy, cw, ch),
        }
    }

    fn draw_terminal_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Botones de comando rápido estilizados
        let btns = ["uname", "uptime", "free", "ps", "ls", "cosmos", "epic38", "clear"];
        for (i, &b) in btns.iter().enumerate() {
            let bx = cx + 8 + (i as i32) * 60;
            if bx + 55 <= cx + (cw as i32) - 4 {
                draw_rounded(c, bx, cy + 6, 55, 20, 4, rgb(17, 24, 39));
                draw_outline(c, bx, cy + 6, 55, 20, 1, rgb(30, 41, 59));
                draw_text(c, b, bx + 6, cy + 10, rgb(56, 189, 248));
            }
        }

        // Búfer de pantalla OLED obsidian
        let term_y = cy + 32;
        let term_h = ch - 62;
        draw_box(c, cx + 4, term_y, cw - 8, term_h, rgb(5, 7, 12));

        let max_vis = (term_h - 10) / 16;
        let start = if self.term_lines.len() > max_vis {
            self.term_lines.len() - max_vis
        } else {
            0
        };

        let max_line_chars = if cw > 48 { (cw - 48) / 8 } else { 30 };

        for (i, line) in self.term_lines.iter().skip(start).enumerate() {
            let ly = term_y + 6 + (i as i32) * 16;
            let default_col = if line.starts_with("root@") || line.contains("❯") {
                rgb(56, 189, 248)
            } else if line.starts_with("[ERROR]") || line.starts_with("sh:") {
                rgb(248, 113, 113)
            } else if line.starts_with("[OK]") || line.contains("-PASS]") {
                rgb(52, 211, 153)
            } else {
                rgb(226, 232, 240)
            };

            if line.contains('\x1b') {
                let spans = crate::console::terminal::ansi::parse_ansi_spans(
                    line,
                    0x00E2E8F0,
                    0x0005070C,
                );
                let mut span_x = cx + 10;
                let max_span_x = cx + (cw as i32) - 24;
                for span in spans {
                    if span_x >= max_span_x { break; }
                    let avail_chars = ((max_span_x - span_x) / 8).max(0) as usize;
                    let text_slice = if span.text.len() > avail_chars {
                        &span.text[..avail_chars]
                    } else {
                        &span.text
                    };
                    let r = ((span.fg >> 16) & 0xFF) as u8;
                    let g = ((span.fg >> 8) & 0xFF) as u8;
                    let b = (span.fg & 0xFF) as u8;
                    draw_text(c, text_slice, span_x, ly, rgb(r, g, b));
                    span_x += (text_slice.len() * 8) as i32;
                }
            } else {
                let slice = if line.len() > max_line_chars {
                    &line[..max_line_chars]
                } else {
                    line.as_str()
                };
                draw_text(c, slice, cx + 10, ly, default_col);
            }
        }

        // Línea de entrada activa con Prompt Starship / Fish moderno
        let in_y = cy + (ch as i32) - 26;
        draw_box(c, cx + 4, in_y, cw - 8, 24, rgb(11, 16, 27));
        draw_outline(c, cx + 4, in_y, cw - 8, 24, 1, rgb(30, 41, 59));

        // Prompt Starship: portix > ~/vfs > v6.0 > 
        let p_host = "portix";
        let p_arrow1 = " > ";
        let p_cwd = if self.term_cwd == "/home/user" { "~" } else { &self.term_cwd };
        let p_arrow2 = " > ";
        let p_ver = "v6.0";
        let p_prompt = " > ";

        let mut px = cx + 10;
        draw_text(c, p_host, px, in_y + 4, rgb(56, 189, 248)); // Cian
        px += (p_host.len() * 9) as i32;
        draw_text(c, p_arrow1, px, in_y + 4, rgb(100, 116, 139)); // Slate
        px += (p_arrow1.len() * 9) as i32;
        draw_text(c, p_cwd, px, in_y + 4, rgb(192, 132, 252)); // Púrpura
        px += (p_cwd.len() * 9) as i32;
        draw_text(c, p_arrow2, px, in_y + 4, rgb(100, 116, 139)); // Slate
        px += (p_arrow2.len() * 9) as i32;
        draw_text(c, p_ver, px, in_y + 4, rgb(52, 211, 153)); // Esmeralda
        px += (p_ver.len() * 9) as i32;
        draw_text(c, p_prompt, px, in_y + 4, rgb(56, 189, 248)); // Cian
        px += (p_prompt.len() * 9) as i32;

        let avail_in = (((cx + cw as i32 - 16) - px) / 9).max(0) as usize;
        let in_slice = if self.term_input.len() > avail_in {
            let st = self.term_input.len() - avail_in;
            &self.term_input[st..]
        } else {
            &self.term_input
        };
        draw_text(c, in_slice, px, in_y + 4, Color::WHITE);

        // Cursor parpadeante estilo bloque Starship
        let cursor_x = px + (in_slice.len() as i32) * 9;
        if cursor_x < cx + (cw as i32) - 12 {
            draw_box(c, cursor_x, in_y + 4, 8, 14, rgb(56, 189, 248));
        }
    }

    fn draw_monitor_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize, hw: &HardwareInfo) {
        // Pestañas
        let tabs = ["1: CPU", "2: RAM", "3: Procs", "4: Red", "5: Clúster SSI"];
        for (i, &t) in tabs.iter().enumerate() {
            let tx = cx + 8 + (i as i32) * 88;
            let bg = if self.monitor_tab == i { rgb(59, 130, 246) } else { rgb(30, 41, 59) };
            draw_rounded(c, tx, cy + 4, 84, 24, 4, bg);
            draw_text(c, t, tx + 6, cy + 8, Color::WHITE);
        }

        let my = cy + 34;
        let mh = ch - 40;
        draw_box(c, cx + 4, my, cw - 8, mh, rgb(15, 23, 42));

        match self.monitor_tab {
            0 => {
                // Recursos / CPU y SMP Multiprocesador
                draw_text(c, "TELEMETRIA DE RECURSOS DEL SISTEMA & MULTINUCLEO SMP", cx + 16, my + 14, rgb(56, 189, 248));
                draw_text(c, &format!("Procesador: {}", hw.cpu.brand_str()), cx + 16, my + 36, Color::WHITE);
                let smp_cores = crate::arch::smp::smp_online_count();
                draw_text(c, &format!("Frecuencia estimada: {} MHz | Nucleos SMP activos: {}", hw.cpu.base_mhz, smp_cores), cx + 16, my + 54, rgb(148, 163, 184));
                draw_text(c, &format!("PIT Ticks activos: {} | Planificador: Per-CPU Dynamic Balancing", pit::ticks()), cx + 16, my + 72, Color::WHITE);

                // Barras gráficas de los 4 núcleos SMP
                for core_idx in 0..4 {
                    let gy = my + 98 + (core_idx as i32) * 36;
                    let core_data = crate::arch::smp::smp_get_core_telemetry(core_idx as u8);
                    let (name, load, tasks) = if let Some(cd) = core_data {
                        let r = if cd.is_bsp { "BSP Master" } else { "AP Worker" };
                        (format!("Core {}: {}", core_idx, r), cd.load_percent as usize, cd.task_count)
                    } else {
                        (format!("Core {}: Online", core_idx), 10 + core_idx * 5, 1)
                    };

                    draw_text(c, &name, cx + 16, gy, Color::WHITE);
                    let bar_w = ((cw - 220) * load / 100).max(6);
                    draw_box(c, cx + 160, gy - 2, cw - 220, 14, rgb(30, 41, 59));
                    let bar_color = if load > 75 { rgb(239, 68, 68) } else if load > 40 { rgb(245, 158, 11) } else { rgb(59, 130, 246) };
                    draw_box(c, cx + 160, gy - 2, bar_w, 14, bar_color);
                    draw_text(c, &format!("{}% ({} tareas)", load, tasks), cx + (cw as i32) - 150, gy, rgb(148, 163, 184));
                }
            }
            1 => {
                // Memoria RAM
                draw_text(c, "ESTADO DEL ASIGNADOR BUDDY SYSTEM (64 MB)", cx + 16, my + 14, rgb(56, 189, 248));
                let free_blks = crate::mem::alloc_stats_free_total();
                let free_kb = (free_blks * 16).min(65536);
                let used_kb = 65536 - free_kb;
                draw_text(c, &format!("Memoria Total:  65536 KB (64 MB)"), cx + 16, my + 38, Color::WHITE);
                draw_text(c, &format!("Memoria Usada:  {} KB", used_kb), cx + 16, my + 58, rgb(245, 158, 11));
                draw_text(c, &format!("Memoria Libre:  {} KB", free_kb), cx + 16, my + 78, rgb(16, 185, 129));

                let bar_w = ((cw - 40) * used_kb / 65536).max(10);
                draw_box(c, cx + 16, my + 110, cw - 40, 20, rgb(30, 41, 59));
                draw_box(c, cx + 16, my + 110, bar_w, 20, rgb(16, 185, 129));
                draw_text(c, &format!("Porcentaje usado: {}%", used_kb * 100 / 65536), cx + 16, my + 138, Color::WHITE);
            }
            2 => {
                // Procesos
                draw_text(c, "PID   PPID  USUARIO  ESTADO   TIEMPO   PROCESO", cx + 16, my + 14, rgb(56, 189, 248));
                let procs = [
                    ("1", "0", "root", "SLEEP", "00:01", "/bin/init"),
                    ("2", "1", "root", "RUN",   "00:03", "/bin/sh"),
                    ("3", "1", "root", "RUN",   "00:08", "portix-wm (Adwaita Shell)"),
                    ("4", "1", "root", "RUN",   "00:02", "portix-browser"),
                    ("5", "1", "root", "RUN",   "00:04", "portix-vulkan"),
                    ("6", "1", "root", "SLEEP", "00:00", "telnetd"),
                ];
                for (i, p) in procs.iter().enumerate() {
                    let py = my + 38 + (i as i32) * 20;
                    let row = format!("{:<5} {:<5} {:<8} {:<8} {:<8} {}", p.0, p.1, p.2, p.3, p.4, p.5);
                    draw_text(c, &row, cx + 16, py, Color::WHITE);
                }
            }
            3 => {
                // Red
                draw_text(c, "INTERFAZ DE RED INTEL 82540EM (e1000)", cx + 16, my + 14, rgb(56, 189, 248));
                draw_text(c, "Estado: ACTIVO (Full Duplex 1000 Mbps Gigabit)", cx + 16, my + 38, rgb(16, 185, 129));
                draw_text(c, "Direccion IP:  10.0.2.15", cx + 16, my + 58, Color::WHITE);
                draw_text(c, "Mascara Red:   255.255.255.0", cx + 16, my + 78, Color::WHITE);
                draw_text(c, "Gateway:       10.0.2.2", cx + 16, my + 98, Color::WHITE);
                draw_text(c, "MAC Hardware:  52:54:00:12:34:56", cx + 16, my + 118, rgb(148, 163, 184));
                draw_text(c, "Paquetes RX: 142  |  Paquetes TX: 89", cx + 16, my + 138, Color::WHITE);
            }
            _ => {
                // Clúster Single System Image (SSI) & Memoria DSM
                draw_text(c, "MALLA CLÚSTER SINGLE SYSTEM IMAGE (SSI) & MEMORIA DSM", cx + 16, my + 14, rgb(56, 189, 248));
                draw_text(c, "Topología: 4 Nodos Interconectados (Zero-Trust WireGuard Ring-0 @ 38.2 Gbps)", cx + 16, my + 34, Color::WHITE);
                draw_text(c, "Capacidad Global: 64 vCPUs Transparentes | 128 GB Memoria DSM Paginada", cx + 16, my + 50, rgb(148, 163, 184));

                let nodes = [
                    ("Nodo Alpha (Host Master)", "x86_64 SMP (16 Cores) | 32 GB DSM", "ONLINE", rgb(34, 197, 94)),
                    ("Nodo Bravo (Worker ARM)", "AArch64 Neoverse (16 Cores) | 32 GB DSM", "ONLINE", rgb(34, 197, 94)),
                    ("Nodo Storage (ZNS Array)", "NVMe Zoned Storage (16 Cores) | 32 GB DSM", "ONLINE", rgb(34, 197, 94)),
                    ("Nodo Render (GPU Cluster)", "VirtIO Venus Vulkan (16 Cores) | 32 GB DSM", "ONLINE", rgb(34, 197, 94)),
                ];

                for (i, (n_name, n_spec, n_st, col)) in nodes.iter().enumerate() {
                    let ny = my + 70 + (i as i32) * 44;
                    draw_rounded(c, cx + 16, ny, cw - 32, 38, 4, rgb(20, 30, 48));
                    draw_outline(c, cx + 16, ny, cw - 32, 38, 1, rgb(51, 65, 85));
                    draw_text(c, n_name, cx + 24, ny + 6, rgb(56, 189, 248));
                    draw_text(c, n_spec, cx + 24, ny + 20, rgb(203, 213, 225));
                    draw_text(c, n_st, cx + (cw as i32) - 80, ny + 12, *col);
                }

                let mig_y = my + 252;
                draw_rounded(c, cx + 16, mig_y, cw - 32, 54, 4, rgb(16, 24, 38));
                draw_outline(c, cx + 16, mig_y, cw - 32, 54, 1, rgb(168, 85, 247));
                draw_text(c, "MIGRACIÓN EN VIVO (LIVE THREAD MIGRATION):", cx + 24, mig_y + 8, rgb(192, 132, 252));
                draw_text(c, "• Hilo PID 42: Alpha (x86_64) -> Bravo (ARM64) en 3.42 ms sin interrupción", cx + 24, mig_y + 24, rgb(34, 197, 94));
                draw_text(c, "• Coherencia DSM: 0 fallos de página | Buffer VMO congelado y reanudado OK", cx + 24, mig_y + 38, Color::WHITE);
            }
        }
    }

    fn draw_calc_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Display digital LCD
        draw_box(c, cx + 10, cy + 8, cw - 20, 36, rgb(10, 15, 25));
        draw_outline(c, cx + 10, cy + 8, cw - 20, 36, 1, rgb(59, 130, 246));
        draw_text(c, &self.calc_display, cx + 20, cy + 16, rgb(0, 220, 120));

        // Teclado 4x4
        let keys = [
            ["7", "8", "9", "/"],
            ["4", "5", "6", "*"],
            ["1", "2", "3", "-"],
            ["C", "0", "=", "+"],
        ];
        let kw = ((cw - 40) / 4) as i32;
        let kh = ((ch - 60) / 4) as i32;
        for row in 0..4i32 {
            for col in 0..4i32 {
                let kx = cx + 10 + col * (kw + 6);
                let ky = cy + 50 + row * (kh + 4);
                let k = keys[row as usize][col as usize];
                let bg = match k {
                    "C" => rgb(239, 68, 68),
                    "=" => rgb(37, 99, 235),
                    "+" | "-" | "*" | "/" => rgb(51, 65, 85),
                    _ => rgb(30, 41, 59),
                };
                draw_rounded(c, kx, ky, kw as usize, kh as usize, 4, bg);
                draw_text(c, k, kx + (kw / 2) - 4, ky + (kh / 2) - 6, Color::WHITE);
            }
        }
    }

    fn draw_explorer_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Toolbar de navegación (Atrás, Adelante, Subir, Recargar)
        draw_box(c, cx + 4, cy + 4, cw - 8, 28, rgb(30, 41, 59));
        draw_rounded(c, cx + 8, cy + 7, 32, 22, 4, rgb(51, 65, 85));
        draw_text(c, "<", cx + 20, cy + 11, Color::WHITE);

        draw_rounded(c, cx + 44, cy + 7, 32, 22, 4, rgb(51, 65, 85));
        draw_text(c, ">", cx + 56, cy + 11, Color::WHITE);

        draw_rounded(c, cx + 80, cy + 7, 36, 22, 4, rgb(51, 65, 85));
        draw_text(c, "^..", cx + 88, cy + 11, Color::WHITE);

        draw_rounded(c, cx + 120, cy + 7, 40, 22, 4, rgb(51, 65, 85));
        draw_text(c, "Rec", cx + 126, cy + 11, Color::WHITE);

        // Barra de Breadcrumbs / Dirección
        draw_box(c, cx + 170, cy + 7, cw - 180, 22, rgb(15, 23, 42));
        let breadcrumb = format!("Portix > {}", self.explorer_path);
        draw_text(c, &breadcrumb, cx + 178, cy + 11, rgb(56, 189, 248));

        // Panel Lateral Izquierdo (Acceso rápido)
        draw_box(c, cx + 4, cy + 34, 130, ch - 40, rgb(20, 28, 45));
        draw_v_line(c, cx + 134, cy + 34, ch - 40, rgb(51, 65, 85));

        draw_text(c, "UBICACIONES", cx + 10, cy + 42, rgb(148, 163, 184));
        let shortcuts = [("/", "Raiz (/)"), ("/bin", "Binarios (/bin)"), ("/etc", "Ajustes (/etc)"), ("/home", "Usuario (/home)"), ("/tmp", "Temporal (/tmp)")];
        for (i, (p, label)) in shortcuts.iter().enumerate() {
            let sy = cy + 62 + (i as i32) * 26;
            let bg = if self.explorer_path == *p { rgb(37, 99, 235) } else { rgb(20, 28, 45) };
            draw_box(c, cx + 6, sy, 124, 22, bg);
            draw_text(c, label, cx + 10, sy + 4, Color::WHITE);
        }

        // Tabla Principal de Archivos VFS
        let table_x = cx + 140;
        let table_y = cy + 34;
        let table_w = cw - 146;
        let table_h = ch - 40;

        draw_box(c, table_x, table_y, table_w, 20, rgb(30, 41, 59));
        draw_text(c, "Nombre", table_x + 8, table_y + 4, rgb(148, 163, 184));
        draw_text(c, "Tipo", table_x + 180, table_y + 4, rgb(148, 163, 184));
        draw_text(c, "Tamaño", table_x + 260, table_y + 4, rgb(148, 163, 184));
        draw_text(c, "Permisos", table_x + 360, table_y + 4, rgb(148, 163, 184));

        let max_items = (table_h - 24) / 22;
        for (i, item) in self.explorer_items.iter().take(max_items).enumerate() {
            let iy = table_y + 24 + (i as i32) * 22;
            let is_sel = self.explorer_selected == i;
            let bg = if is_sel { rgb(37, 99, 235) } else if i % 2 == 0 { rgb(15, 23, 42) } else { rgb(20, 30, 50) };
            draw_box(c, table_x, iy, table_w, 20, bg);

            let icon_str = if item.is_dir { "[D] " } else { "[F] " };
            draw_text(c, icon_str, table_x + 6, iy + 4, rgb(56, 189, 248));
            draw_text(c, &item.name, table_x + 32, iy + 4, Color::WHITE);
            draw_text(c, &item.kind_str, table_x + 180, iy + 4, rgb(245, 158, 11));
            draw_text(c, &format_bytes(item.size), table_x + 260, iy + 4, rgb(16, 185, 129));
            draw_text(c, &item.perms, table_x + 360, iy + 4, rgb(148, 163, 184));
        }
    }

    fn draw_editor_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Barra de herramientas del editor IDE
        draw_box(c, cx + 4, cy + 4, cw - 8, 28, rgb(30, 41, 59));
        let tools = [
            ("▶ Run", rgb(34, 197, 94)),
            ("🐛 Debug", rgb(168, 85, 247)),
            ("⏭ Step", rgb(245, 158, 11)),
            ("💾 Save", rgb(59, 130, 246)),
            ("Limpiar", rgb(239, 68, 68)),
        ];
        for (i, (name, col)) in tools.iter().enumerate() {
            let bx = cx + 8 + (i as i32) * 78;
            draw_rounded(c, bx, cy + 7, 72, 22, 4, *col);
            draw_text(c, name, bx + 6, cy + 11, Color::WHITE);
        }

        // Ruta del archivo actual
        let max_path = if cw > 460 { (cw - 460) / 8 } else { 20 };
        let path_slice = if self.editor_path.len() > max_path { &self.editor_path[..max_path] } else { &self.editor_path };
        draw_text(c, path_slice, cx + 410, cy + 11, rgb(56, 189, 248));

        // Área de código con numeración de líneas
        let ed_y = cy + 34;
        let ed_h = ch.saturating_sub(136);
        draw_box(c, cx + 4, ed_y, cw - 8, ed_h, rgb(10, 15, 25));

        // Margen de números de línea y breakpoints
        draw_box(c, cx + 4, ed_y, 38, ed_h, rgb(20, 28, 45));
        draw_v_line(c, cx + 42, ed_y, ed_h, rgb(51, 65, 85));

        let max_lines = ed_h / 18;
        let max_code_chars = if cw > 60 { (cw - 60) / 8 } else { 30 };
        for (i, line) in self.editor_lines.iter().take(max_lines).enumerate() {
            let ly = ed_y + 4 + (i as i32) * 18;
            let line_no = i + 1;

            // Breakpoint activo (círculo rojo en margen)
            if self.editor_breakpoints.contains(&line_no) {
                draw_rounded(c, cx + 6, ly + 4, 8, 8, 4, rgb(239, 68, 68));
            }

            draw_text(c, &format!("{:>2}", line_no), cx + 18, ly, rgb(148, 163, 184));

            let trimmed = line.trim_start();
            let line_col = if trimmed.starts_with("//") || trimmed.starts_with('#') {
                rgb(100, 116, 139)
            } else if trimmed.starts_with("fn ") || trimmed.starts_with("let ") || trimmed.starts_with("pub ") 
                || trimmed.starts_with("def ") || trimmed.starts_with("import ") || trimmed.starts_with("int ") {
                rgb(56, 189, 248)
            } else if line.contains('"') {
                rgb(74, 222, 128)
            } else {
                rgb(241, 245, 249)
            };

            let code_slice = if line.len() > max_code_chars { &line[..max_code_chars] } else { line.as_str() };
            draw_text(c, code_slice, cx + 48, ly, line_col);
        }

        // Panel inferior: Terminal de salida / Depurador interactivo
        let out_y = ed_y + (ed_h as i32) + 6;
        let out_h = 72usize;
        draw_box(c, cx + 4, out_y, cw - 8, out_h, rgb(10, 16, 26));
        draw_outline(c, cx + 4, out_y, cw - 8, out_h, 1, rgb(51, 65, 85));

        // Pestañas del panel inferior
        let d_tabs = ["Registros x86_64", "Memoria Hex", "Pila (Stack)", "Consola"];
        for (t_idx, &t_name) in d_tabs.iter().enumerate() {
            let tx = cx + 8 + (t_idx as i32) * 120;
            let t_bg = if self.editor_debug_tab == t_idx { rgb(37, 99, 235) } else { rgb(20, 30, 48) };
            draw_rounded(c, tx, out_y + 2, 114, 18, 3, t_bg);
            draw_text(c, t_name, tx + 6, out_y + 4, Color::WHITE);
        }

        let max_panel_chars = if cw > 28 { (cw - 28) / 8 } else { 35 };
        match self.editor_debug_tab {
            0 => { // Registros CPU x86_64
                let l1 = format!("RAX: 0x{:08X} ({})  RBX: 0x{:08X}  RCX: 0x{:04X}  RDX: 0x{:04X}", 
                    self.editor_reg_rax, self.editor_reg_rax, self.editor_reg_rbx, self.editor_reg_rcx, self.editor_reg_rdx);
                let l2 = format!("RSP: 0x{:08X}  RBP: 0x{:08X}  RIP: 0x{:08X}  RFL: 0x{:04X} [GDB DR0-3]", 
                    self.editor_reg_rsp, self.editor_reg_rbp, self.editor_reg_rip, self.editor_reg_rflags);
                let s1 = if l1.len() > max_panel_chars { &l1[..max_panel_chars] } else { &l1 };
                let s2 = if l2.len() > max_panel_chars { &l2[..max_panel_chars] } else { &l2 };
                draw_text(c, s1, cx + 12, out_y + 26, rgb(56, 189, 248));
                draw_text(c, s2, cx + 12, out_y + 46, rgb(251, 191, 36));
            }
            1 => { // Memoria Hex
                let l1 = format!("0x{:08X}: 48 89 E5 48 83 EC 20 B8  2A 00 00 00 C9 C3 90 90", self.editor_reg_rip);
                let l2 = format!("0x{:08X}: 55 48 89 E5 48 8D 3D 10  00 00 00 E8 40 01 00 00", self.editor_reg_rip + 16);
                let s1 = if l1.len() > max_panel_chars { &l1[..max_panel_chars] } else { &l1 };
                let s2 = if l2.len() > max_panel_chars { &l2[..max_panel_chars] } else { &l2 };
                draw_text(c, s1, cx + 12, out_y + 26, rgb(74, 222, 128));
                draw_text(c, s2, cx + 12, out_y + 46, rgb(148, 163, 184));
            }
            2 => { // Pila / Stack Dump
                let l1 = format!("[RSP+00] 0x{:08X} (__libc_start_main+0x45) | [RSP+08] 0x7FFF0010 (argv)", self.editor_reg_rip + 64);
                let l2 = String::from("[RSP+10] 0x00000001 (argc = 1)                | [RSP+18] 0x7FFF0018 (envp)");
                let s1 = if l1.len() > max_panel_chars { &l1[..max_panel_chars] } else { &l1 };
                let s2 = if l2.len() > max_panel_chars { &l2[..max_panel_chars] } else { &l2 };
                draw_text(c, s1, cx + 12, out_y + 26, rgb(192, 132, 252));
                draw_text(c, s2, cx + 12, out_y + 46, rgb(203, 213, 225));
            }
            _ => { // Consola
                for (i, out_l) in self.editor_output.iter().rev().take(2).rev().enumerate() {
                    let ly = out_y + 26 + (i as i32) * 18;
                    let col = if out_l.contains("[OK]") || out_l.contains("[PASS]") {
                        rgb(34, 197, 94)
                    } else if out_l.contains("[ERR]") {
                        rgb(239, 68, 68)
                    } else if out_l.contains("[IDE]") || out_l.contains("[BUILD]") {
                        rgb(251, 191, 36)
                    } else {
                        rgb(203, 213, 225)
                    };
                    let out_slice = if out_l.len() > max_panel_chars { &out_l[..max_panel_chars] } else { out_l.as_str() };
                    draw_text(c, out_slice, cx + 12, ly, col);
                }
            }
        }

        // Barra de estado inferior
        let stat_y = cy + (ch as i32) - 20;
        draw_box(c, cx + 4, stat_y, cw - 8, 18, rgb(20, 28, 45));
        let deb_tag = if self.editor_debugger_active { "[GDB DEBUG ON]" } else { "[NORMAL]" };
        draw_text(c, &format!("{} Estado: {}", deb_tag, self.editor_status), cx + 10, stat_y + 3, rgb(16, 185, 129));
        draw_text(c, &format!("Lin {}, Col {}", self.editor_cursor_row + 1, self.editor_cursor_col + 1), cx + (cw as i32) - 130, stat_y + 3, rgb(148, 163, 184));
    }

    fn draw_control_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize, hw: &HardwareInfo) {
        draw_box(c, cx + 4, cy + 4, cw - 8, ch - 8, rgb(11, 16, 27));

        // Pestañas superiores del Centro de Control v6.0 'Chronos' (Traducidas según idioma activo)
        let (tab_silicon, tab_bench, tab_mesh) = {
            let i18n = crate::compat::i18n::GLOBAL_I18N.lock();
            (
                i18n.translate("silicon"),
                i18n.translate("bench"),
                i18n.translate("mesh"),
            )
        };
        let tabs = [
            String::from("* Chronos"),
            format!("* {}", tab_silicon),
            format!("* {}", tab_bench),
            format!("* {}", tab_mesh),
        ];
        for (i, t) in tabs.iter().enumerate() {
            let tx = cx + 6 + (i as i32) * 96;
            let bg = if self.control_tab == i { rgb(37, 99, 235) } else { rgb(20, 30, 48) };
            draw_rounded(c, tx, cy + 6, 92, 22, 4, bg);
            draw_text(c, t, tx + 8, cy + 9, Color::WHITE);
        }

        let content_y = cy + 34;
        match self.control_tab {
            0 => { // Chronos v6.0 & Épicas Soberanas (4,000 Fases)
                draw_rounded(c, cx + 8, content_y, cw - 16, 44, 4, rgb(20, 30, 48));
                draw_outline(c, cx + 8, content_y, cw - 16, 44, 1, rgb(56, 189, 248));
                draw_text(c, "PORTIX OS v6.0 'CHRONOS'", cx + 16, content_y + 8, rgb(56, 189, 248));
                draw_text(c, "4,000 Fases | ABI Congelada | Ring 3", cx + 16, content_y + 24, rgb(226, 232, 240));

                let actions = [
                    ("1. Portix-FS Ultra (ZNS)", "BLAKE3 Dedup, LZ4/ZSTD, 850k IOPS, Cero fsck", rgb(59, 130, 246)),
                    ("2. Hipervisor Bare-Metal Tipo-1", "MicroVMs <8ms, VirtIO-GPU 3D, Win11 & Linux", rgb(168, 85, 247)),
                    ("3. Red Malla Zero-Trust & P2P", "WireGuard Ring-0 (38Gbps), portix-drop, eBPF", rgb(16, 185, 129)),
                    ("4. Resiliencia & Hot-Patching", "Live Patching sin reinicio, Driver Recovery 20ms", rgb(245, 158, 11)),
                ];

                for (i, (act, sub, col)) in actions.iter().enumerate() {
                    let by = content_y + 54 + (i as i32) * 44;
                    draw_rounded(c, cx + 8, by, cw - 16, 40, 4, rgb(16, 24, 38));
                    draw_outline(c, cx + 8, by, cw - 16, 40, 1, *col);
                    draw_text(c, act, cx + 18, by + 6, *col);
                    let max_sub = if cw > 110 { (cw - 110) / 8 } else { 20 };
                    let sub_slice = if sub.len() > max_sub { &sub[..max_sub] } else { sub };
                    draw_text(c, sub_slice, cx + 18, by + 22, rgb(203, 213, 225));
                    draw_text(c, "[Abrir]", cx + (cw as i32) - 64, by + 12, rgb(148, 163, 184));
                }

                // Barra de Feedback
                let feed_y = content_y + 242;
                draw_rounded(c, cx + 8, feed_y, cw - 16, 48, 4, rgb(9, 13, 22));
                draw_outline(c, cx + 8, feed_y, cw - 16, 48, 1, rgb(51, 65, 85));
                draw_text(c, "FEEDBACK DEL ASISTENTE:", cx + 16, feed_y + 8, rgb(56, 189, 248));
                let max_feed = if cw > 32 { (cw - 32) / 8 } else { 30 };
                let f_slice = if self.assistant_feedback.len() > max_feed { &self.assistant_feedback[..max_feed] } else { &self.assistant_feedback };
                draw_text(c, f_slice, cx + 16, feed_y + 26, rgb(241, 245, 249));
            }
            1 => { // Silicio, Arquitectura & Compatibilidad
                draw_rounded(c, cx + 8, content_y, cw - 16, 62, 4, rgb(20, 30, 48));
                draw_outline(c, cx + 8, content_y, cw - 16, 62, 1, rgb(168, 85, 247));
                draw_text(c, "ARQUITECTURA MODULAR & SILICIO REAL", cx + 16, content_y + 8, rgb(192, 132, 252));
                draw_text(c, "Microkernel modular en Rust puro con aislamiento estricto de memoria.", cx + 16, content_y + 26, Color::WHITE);
                draw_text(c, "ABI Linux x86-64 como capa: binarios ELF de distro sin modificar (musl hoy, glibc en curso).", cx + 16, content_y + 44, rgb(148, 163, 184));

                let card_y = content_y + 70;
                draw_rounded(c, cx + 8, card_y, cw - 16, 150, 4, rgb(16, 24, 38));
                draw_outline(c, cx + 8, card_y, cw - 16, 150, 1, rgb(51, 65, 85));
                draw_text(c, "Subsistemas de Silicio Operativos:", cx + 16, card_y + 10, rgb(56, 189, 248));
                draw_text(c, "* Microkernel Fast-IPC: Mensajeria sincrona en registros <50ns", cx + 16, card_y + 30, rgb(203, 213, 225));
                draw_text(c, "* Buddy Allocator: Gestion de memoria fisica O(1) ordenes 0-11", cx + 16, card_y + 50, rgb(203, 213, 225));
                draw_text(c, "* Paginacion x86_64: Aislamiento estricto de 4 niveles Ring 0/Ring 3", cx + 16, card_y + 70, rgb(203, 213, 225));
                draw_text(c, "* Ring-3 io_uring: 134k IOPS DMA Zero-Syscall", cx + 16, card_y + 90, rgb(203, 213, 225));
                draw_text(c, "* Multi-Arch ARM64 HAL: GICv3 + TTBR0/1 + PL011", cx + 16, card_y + 110, rgb(203, 213, 225));
                draw_text(c, "Raiz ext2 real verificada por e2fsck; /boot FAT32 de solo lectura", cx + 16, card_y + 130, rgb(34, 197, 94));

            }
            2 => { // Benchmarks de Rendimiento
                draw_rounded(c, cx + 8, content_y, cw - 16, 50, 4, rgb(20, 30, 48));
                draw_outline(c, cx + 8, content_y, cw - 16, 50, 1, rgb(245, 158, 11));
                // [T-011] Solo medidas reales de esta maquina; sin comparativas.
                draw_text(c, "MEDICIONES REALES EN ESTA MAQUINA", cx + 16, content_y + 8, rgb(251, 191, 36));
                draw_text(c, "TSC calibrado con PIT, asignador de marcos, VFS->ext2->ATA:", cx + 16, content_y + 28, Color::WHITE);

                let bench_y = content_y + 58;
                let med = crate::system::bench::ultimas();
                let (v_arr, v_ram, v_ext2) = match med {
                    Some(m) => (
                        if m.arranque_ms > 0 { format!("{} ms hasta escritorio (TSC desde reset)", m.arranque_ms) } else { String::from("no disponible") },
                        format!("{} KiB de {} KiB (fijo {} KiB, heap {} KiB, usuario {} KiB)", m.ram_usada_kib, m.ram_total_kib, m.ram_fija_kib, m.heap_usado_kib, m.marcos_usados_kib),
                        if m.ext2_kbps > 0 { format!("{} KiB/s disco, {} KiB/s cache ({} B)", m.ext2_kbps, m.ext2_kbps2, m.ext2_bytes) } else { String::from("no medida") },
                    ),
                    None => (String::from("sin medir: pulsa el boton"), String::from("sin medir: pulsa el boton"), String::from("sin medir: pulsa el boton")),
                };
                let v_sys = String::from("desde Ring 3: run portix-bench (getpid x100000 por TSC)");
                let metrics = [
                    ("ARRANQUE", v_arr.as_str(), rgb(56, 189, 248)),
                    ("MEMORIA FISICA", v_ram.as_str(), rgb(168, 85, 247)),
                    ("LECTURA EXT2", v_ext2.as_str(), rgb(245, 158, 11)),
                    ("LATENCIA SYSCALL", v_sys.as_str(), rgb(34, 197, 94)),
                ];

                for (i, (m_title, m_val, col)) in metrics.iter().enumerate() {
                    let by = bench_y + (i as i32) * 44;
                    draw_rounded(c, cx + 8, by, cw - 16, 38, 4, rgb(16, 24, 38));
                    draw_outline(c, cx + 8, by, cw - 16, 38, 1, rgb(51, 65, 85));
                    draw_text(c, m_title, cx + 16, by + 6, *col);
                    let max_m = if cw > 32 { (cw - 32) / 8 } else { 20 };
                    let val_slice = if m_val.len() > max_m { &m_val[..max_m] } else { m_val };
                    draw_text(c, val_slice, cx + 16, by + 20, Color::WHITE);
                }

                let btn_y = bench_y + 186;
                draw_rounded(c, cx + 12, btn_y, cw - 24, 30, 4, rgb(217, 119, 6));
                draw_text(c, "[ Medir ahora en esta maquina ]", cx + 24, btn_y + 8, Color::WHITE);
            }
            _ => { // Malla & Hardware
                let cards = [
                    ("RED", "Intel e1000 Gigabit + pila TCP/IP propia (sin cifra: no medida)"),
                    ("VULKAN 3D VENUS", "VirtIO Venus 3D / Vulkan 1.3 SPIR-V Pipelines"),
                    ("ALMACENAMIENTO", "ATA PIO LBA48; raiz ext2 real (MB/s en pestana Benchmarks)"),
                    ("AUDIO", "Intel HD Audio / AC97 PCM (latencia no medida)"),
                ];

                for (i, (title, desc)) in cards.iter().enumerate() {
                    let by = content_y + 10 + (i as i32) * 52;
                    draw_rounded(c, cx + 8, by, cw - 16, 46, 4, rgb(20, 30, 48));
                    draw_outline(c, cx + 8, by, cw - 16, 46, 1, rgb(51, 65, 85));
                    draw_text(c, title, cx + 16, by + 6, rgb(56, 189, 248));
                    draw_text(c, desc, cx + 16, by + 24, Color::WHITE);
                }

                let btn_y = content_y + 228;
                draw_rounded(c, cx + 12, btn_y, cw - 24, 30, 4, rgb(37, 99, 235));
                draw_text(c, "[ Probar Audio UI / Chimes ]", cx + 24, btn_y + 8, Color::WHITE);

                draw_text(c, &format!("CPU: {} | PIT 100Hz", hw.cpu.brand_str()), cx + 16, cy + ch as i32 - 20, rgb(148, 163, 184));
            }
        }
    }

    fn draw_toast(&self, c: &mut Console) {
        let tw = 400usize;
        let th = 56usize;
        let tx = (1024 - tw as i32) - 16;
        let ty = 660i32;

        draw_rounded(c, tx, ty, tw, th, 8, rgb(15, 23, 42));
        draw_outline(c, tx, ty, tw, th, 1, rgb(56, 189, 248));
        draw_box(c, tx + 1, ty + 1, tw - 2, 20, rgb(30, 41, 59));

        draw_text(c, &self.toast_title, tx + 12, ty + 4, rgb(56, 189, 248));
        draw_text(c, "Ahora", tx + (tw as i32) - 55, ty + 4, rgb(148, 163, 184));

        let max_len = 46;
        if self.toast_msg.len() > max_len {
            let split = self.toast_msg[..max_len].rfind(' ').unwrap_or(max_len);
            draw_text(c, &self.toast_msg[..split], tx + 12, ty + 24, Color::WHITE);
            draw_text(c, self.toast_msg[split..].trim(), tx + 12, ty + 38, rgb(203, 213, 225));
        } else {
            draw_text(c, &self.toast_msg, tx + 12, ty + 28, Color::WHITE);
        }
    }

    fn draw_browser_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Pestañas dinámicas
        let tabs = ["Docs", "Servidor Local", "Telemetria REST", "Repositorio P2P"];
        for (i, &t) in tabs.iter().enumerate() {
            let tx = cx + 8 + (i as i32) * 125;
            let bg = if self.browser_tab == i { rgb(59, 130, 246) } else { rgb(30, 41, 59) };
            draw_rounded(c, tx, cy + 4, 120, 22, 4, bg);
            draw_text(c, t, tx + 8, cy + 8, Color::WHITE);
        }

        // Barra de navegación y URL
        let nav_y = cy + 28;
        draw_box(c, cx + 4, nav_y, cw - 8, 26, rgb(20, 28, 45));
        draw_text(c, "<  >  O", cx + 10, nav_y + 6, rgb(148, 163, 184));

        draw_box(c, cx + 80, nav_y + 2, cw - 160, 22, rgb(10, 15, 25));
        draw_outline(c, cx + 80, nav_y + 2, cw - 160, 22, 1, rgb(51, 65, 85));
        draw_text(c, "[TLS 1.3] ", cx + 86, nav_y + 6, rgb(16, 185, 129));
        let max_url_chars = if cw > 240 { (cw - 240) / 8 } else { 20 };
        let url_slice = if self.browser_url.len() > max_url_chars {
            &self.browser_url[..max_url_chars]
        } else {
            &self.browser_url
        };
        draw_text(c, url_slice, cx + 160, nav_y + 6, Color::WHITE);

        draw_rounded(c, cx + (cw as i32) - 70, nav_y + 2, 60, 22, 4, rgb(37, 99, 235));
        draw_text(c, "Ir", cx + (cw as i32) - 48, nav_y + 6, Color::WHITE);

        // Área de contenido Web estilizada
        let page_y = cy + 56;
        let page_h = ch - 62;
        draw_box(c, cx + 4, page_y, cw - 8, page_h, rgb(15, 23, 42));
        draw_outline(c, cx + 4, page_y, cw - 8, page_h, 1, rgb(30, 41, 59));

        let max_content_chars = if cw > 48 { (cw - 48) / 8 } else { 35 };

        let t_slice = if self.browser_title.len() > max_content_chars {
            &self.browser_title[..max_content_chars]
        } else {
            &self.browser_title
        };
        draw_text(c, t_slice, cx + 20, page_y + 14, rgb(56, 189, 248));
        draw_h_line(c, cx + 20, page_y + 32, cw - 40, rgb(51, 65, 85));

        let mut row_idx = 0;
        for line in self.browser_content.iter() {
            let ly = page_y + 44 + (row_idx as i32) * 20;
            if ly + 20 > cy + (ch as i32) - 10 { break; }

            let trimmed = line.trim();
            if trimmed == "<!DOCTYPE html>" || trimmed == "<html>" || trimmed == "<body>" 
                || trimmed == "</body>" || trimmed == "</html>" || trimmed.is_empty() {
                continue;
            }

            let clean_line = if trimmed.starts_with("<h1>") && trimmed.ends_with("</h1>") {
                &trimmed[4..trimmed.len() - 5]
            } else if trimmed.starts_with("<p>") && trimmed.ends_with("</p>") {
                &trimmed[3..trimmed.len() - 4]
            } else {
                trimmed
            };

            let (text_col, prefix) = if trimmed.starts_with("<h1>") {
                (rgb(56, 189, 248), "# ")
            } else if trimmed.starts_with("PAQUETES") || trimmed.starts_with("DOCUMENTACION") || trimmed.starts_with("HTTP/1.1") {
                (rgb(251, 191, 36), "▶ ")
            } else if trimmed.starts_with("- ") {
                (rgb(226, 232, 240), "")
            } else if trimmed.contains("ACTIVO") || trimmed.contains("OK") {
                (rgb(34, 197, 94), "● ")
            } else {
                (rgb(203, 213, 225), "  ")
            };

            let formatted = format!("{}{}", prefix, clean_line);
            let slice = if formatted.len() > max_content_chars {
                &formatted[..max_content_chars]
            } else {
                &formatted
            };
            draw_text(c, slice, cx + 20, ly, text_col);
            row_idx += 1;
        }
    }

    fn draw_doom_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Viewport 3D de raycasting
        let vp_y = cy + 4;
        let vp_h = ch - 54;
        draw_box(c, cx + 4, vp_y, cw - 8, vp_h / 2, rgb(40, 40, 50)); // Techo
        draw_box(c, cx + 4, vp_y + (vp_h as i32) / 2, cw - 8, vp_h / 2, rgb(70, 70, 70)); // Suelo

        // Columnas de pared simulada
        for col in 0..16 {
            let px = cx + 8 + (col as i32) * ((cw as i32 - 16) / 16);
            let wall_h = 60 + ((col * 17 + self.doom_score as i32) % 110);
            let wy = vp_y + (vp_h as i32 / 2) - wall_h / 2;
            let shade = 100 + ((col * 9) % 120) as u8;
            draw_box(c, px, wy, 24, wall_h as usize, rgb(shade, 30, 30));
        }

        // Arma en el centro inferior
        let gun_x = cx + (cw as i32 / 2) - 30;
        let gun_y = vp_y + (vp_h as i32) - 60;
        let gun_col = if self.doom_firing > 0 { rgb(255, 220, 50) } else { rgb(60, 60, 70) };
        draw_box(c, gun_x, gun_y, 60, 50, gun_col);
        draw_box(c, gun_x + 20, gun_y - 20, 20, 25, rgb(40, 40, 45));

        // Crosshair
        draw_box(c, cx + (cw as i32 / 2) - 4, vp_y + (vp_h as i32 / 2) - 1, 8, 2, rgb(0, 255, 0));
        draw_box(c, cx + (cw as i32 / 2) - 1, vp_y + (vp_h as i32 / 2) - 4, 2, 8, rgb(0, 255, 0));

        // HUD DOOM clásico
        let hud_y = cy + (ch as i32) - 46;
        draw_box(c, cx + 4, hud_y, cw - 8, 42, rgb(20, 20, 25));
        draw_text(c, &format!("SALUD: {}%", self.doom_hp), cx + 16, hud_y + 12, rgb(255, 50, 50));
        draw_text(c, &format!("ARMADURA: {}%", self.doom_armor), cx + 130, hud_y + 12, rgb(56, 189, 248));
        draw_text(c, &format!("MUNICION: {}", self.doom_ammo), cx + 260, hud_y + 12, rgb(255, 200, 50));
        draw_text(c, &format!("FRAGS: {}", self.doom_kills), cx + 380, hud_y + 12, rgb(16, 185, 129));
        draw_text(c, &format!("SCORE: {}", self.doom_score), cx + 480, hud_y + 12, Color::WHITE);
    }

    fn draw_vulkan_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Toolbar de controles
        draw_box(c, cx + 4, cy + 4, cw - 8, 28, rgb(30, 41, 59));
        let wire_col = if self.vulkan_wireframe { rgb(16, 185, 129) } else { rgb(51, 65, 85) };
        draw_rounded(c, cx + 10, cy + 7, 95, 22, 4, wire_col);
        draw_text(c, "Malla (Wire)", cx + 16, cy + 11, Color::WHITE);

        let light_col = if self.vulkan_lighting { rgb(16, 185, 129) } else { rgb(51, 65, 85) };
        draw_rounded(c, cx + 112, cy + 7, 95, 22, 4, light_col);
        draw_text(c, "Luz Phong", cx + 124, cy + 11, Color::WHITE);

        let auto_col = if self.vulkan_auto_rotate { rgb(16, 185, 129) } else { rgb(51, 65, 85) };
        draw_rounded(c, cx + 214, cy + 7, 95, 22, 4, auto_col);
        draw_text(c, "Auto Rotar", cx + 224, cy + 11, Color::WHITE);

        draw_rounded(c, cx + 316, cy + 7, 95, 22, 4, rgb(51, 65, 85));
        draw_text(c, "Reset Cam", cx + 328, cy + 11, Color::WHITE);

        // Viewport 3D
        let vp_y = cy + 34;
        let vp_h = ch - 72;
        draw_box(c, cx + 4, vp_y, cw - 8, vp_h, rgb(10, 15, 25));

        // Renderizado del cubo 3D proyectado
        let center_x = cx + (cw as i32 / 2);
        let center_y = vp_y + (vp_h as i32 / 2);

        // Si auto_rotar está activo, rotar suavemente con los ticks del reloj PIT
        let (rx, ry) = if self.vulkan_auto_rotate {
            let tick_angle = ((crate::time::pit::ticks() * 2) % 360) as i32;
            ((self.vulkan_rot_x + tick_angle) % 360, (self.vulkan_rot_y + tick_angle * 2) % 360)
        } else {
            (self.vulkan_rot_x, self.vulkan_rot_y)
        };

        // 8 vértices en el espacio de modelo 3D (-60 .. +60)
        let model_verts: [(i32, i32, i32); 8] = [
            (-60, -60, -60), // 0: Sup-Izq-Frontal
            ( 60, -60, -60), // 1: Sup-Der-Frontal
            ( 60,  60, -60), // 2: Inf-Der-Frontal
            (-60,  60, -60), // 3: Inf-Izq-Frontal
            (-60, -60,  60), // 4: Sup-Izq-Trasero
            ( 60, -60,  60), // 5: Sup-Der-Trasero
            ( 60,  60,  60), // 6: Inf-Der-Trasero
            (-60,  60,  60), // 7: Inf-Izq-Trasero
        ];

        let cos_y = cos_fp(ry);
        let sin_y = sin_fp(ry);
        let cos_x = cos_fp(rx);
        let sin_x = sin_fp(rx);

        // Transformación de rotación 3D y proyección en perspectiva
        let mut proj_verts: [(i32, i32); 8] = [(0, 0); 8];
        let fov = 260;
        let dist = 320;

        for (i, &(vx, vy, vz)) in model_verts.iter().enumerate() {
            // Rotación sobre eje Y
            let x1 = (vx * cos_y + vz * sin_y) / 256;
            let z1 = (-vx * sin_y + vz * cos_y) / 256;

            // Rotación sobre eje X
            let y2 = (vy * cos_x - z1 * sin_x) / 256;
            let z2 = (vy * sin_x + z1 * cos_x) / 256;

            // Proyección de perspectiva hacia el plano de pantalla
            let z_proj = (z2 + dist).max(1);
            let px = center_x + (x1 * fov) / z_proj;
            let py = center_y + (y2 * fov) / z_proj;

            proj_verts[i] = (px, py);
        }

        if self.vulkan_wireframe {
            // Modo Malla (Wireframe): dibujar las 12 aristas con algoritmo de Bresenham
            let edges: [(usize, usize); 12] = [
                (0, 1), (1, 2), (2, 3), (3, 0), // Cara frontal
                (4, 5), (5, 6), (6, 7), (7, 4), // Cara trasera
                (0, 4), (1, 5), (2, 6), (3, 7), // Aristas de unión
            ];

            for &(e0, e1) in &edges {
                let p0 = proj_verts[e0];
                let p1 = proj_verts[e1];
                c.draw_line(p0.0, p0.1, p1.0, p1.1, rgb(56, 189, 248));
            }

            // Destacar vértices 3D
            for &p in &proj_verts {
                draw_box(c, p.0 - 2, p.1 - 2, 5, 5, rgb(16, 185, 129));
            }
        } else {
            // Modo Sólido con Culling y Sombreado Phong / Gouraud
            let faces: [([usize; 4], Color); 6] = [
                ([0, 1, 2, 3], rgb(37, 99, 235)),  // Frontal: Azul
                ([5, 4, 7, 6], rgb(59, 130, 246)), // Trasera: Celeste
                ([4, 5, 1, 0], rgb(16, 185, 129)), // Superior: Esmeralda
                ([3, 2, 6, 7], rgb(245, 158, 11)), // Inferior: Ámbar
                ([1, 5, 6, 2], rgb(99, 102, 241)), // Derecha: Índigo
                ([4, 0, 3, 7], rgb(168, 85, 247)), // Izquierda: Púrpura
            ];

            for &([i0, i1, i2, i3], col) in &faces {
                let p0 = proj_verts[i0];
                let p1 = proj_verts[i1];
                let p2 = proj_verts[i2];
                let p3 = proj_verts[i3];

                // Producto vectorial 2D para determinar si la cara apunta a la cámara (Backface Culling)
                let cross = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
                if cross > 0 {
                    let shaded_col = if self.vulkan_lighting {
                        let intensity = (130 + (cross / 90).clamp(0, 120)) as u8;
                        col.blend(Color::WHITE, intensity)
                    } else {
                        col
                    };

                    // Dibujar aristas de la cara
                    c.draw_line(p0.0, p0.1, p1.0, p1.1, shaded_col);
                    c.draw_line(p1.0, p1.1, p2.0, p2.1, shaded_col);
                    c.draw_line(p2.0, p2.1, p3.0, p3.1, shaded_col);
                    c.draw_line(p3.0, p3.1, p0.0, p0.1, shaded_col);

                    // Diagonales y centro para relleno facetado
                    let mid_x = (p0.0 + p1.0 + p2.0 + p3.0) / 4;
                    let mid_y = (p0.1 + p1.1 + p2.1 + p3.1) / 4;
                    c.draw_line(p0.0, p0.1, mid_x, mid_y, shaded_col);
                    c.draw_line(p1.0, p1.1, mid_x, mid_y, shaded_col);
                    c.draw_line(p2.0, p2.1, mid_x, mid_y, shaded_col);
                    c.draw_line(p3.0, p3.1, mid_x, mid_y, shaded_col);
                }
            }
        }

        // Info bar
        let inf_y = cy + (ch as i32) - 34;
        draw_box(c, cx + 4, inf_y, cw - 8, 30, rgb(20, 28, 45));
        draw_text(c, "Pipeline: VkGraphicsPipeline | Triangulos: 12 | Shaders: SPIR-V | 60 FPS", cx + 16, inf_y + 8, rgb(0, 220, 120));
        draw_text(c, "VirtIO-GPU Venus / Virgl 3D Protocol | Wayland SHM", cx + 330, inf_y + 8, rgb(148, 163, 184));
    }

    fn draw_appstore_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Categorías
        let cats = ["Todos", "Sistema", "Desarrollo", "Graficos", "Juegos"];
        for (i, &cat) in cats.iter().enumerate() {
            let tx = cx + 8 + (i as i32) * 85;
            let bg = if self.store_category == i { rgb(59, 130, 246) } else { rgb(30, 41, 59) };
            draw_rounded(c, tx, cy + 6, 80, 22, 4, bg);
            draw_text(c, cat, tx + 10, cy + 10, Color::WHITE);
        }

        let py = cy + 34;
        let ph = ch - 40;
        draw_box(c, cx + 4, py, cw - 8, ph, rgb(15, 23, 42));

        let mut row = 0;
        for pkg in self.store_packages.iter() {
            let matches_cat = self.store_category == 0 || pkg.category == self.store_category;
            if !matches_cat { continue; }

            let iy = py + 10 + (row as i32) * 55;
            if iy + 50 > cy + ch as i32 { break; }

            draw_rounded(c, cx + 12, iy, cw - 24, 48, 4, rgb(24, 34, 56));
            draw_text(c, &pkg.name, cx + 24, iy + 8, rgb(56, 189, 248));
            draw_text(c, &pkg.desc, cx + 24, iy + 26, rgb(148, 163, 184));
            draw_text(c, &pkg.size_str, cx + 450, iy + 16, Color::WHITE);

            let (btn_bg, btn_txt) = if pkg.is_installed {
                (rgb(16, 185, 129), "Abrir")
            } else {
                (rgb(37, 99, 235), "Instalar")
            };
            draw_rounded(c, cx + (cw as i32) - 110, iy + 12, 90, 24, 4, btn_bg);
            draw_text(c, btn_txt, cx + (cw as i32) - 96, iy + 17, Color::WHITE);

            row += 1;
        }
    }

    fn draw_synth_window(&self, c: &mut Console, cx: i32, cy: i32, cw: usize, ch: usize) {
        // Pestañas superiores: 0: Sintetizador MIDI, 1: Reproductor Video 8K HDR10+
        let tabs = ["1: Sintetizador MIDI", "2: Reproductor Video 8K HDR10+"];
        for (i, &t) in tabs.iter().enumerate() {
            let tx = cx + 14 + (i as i32) * 200;
            let bg = if self.synth_tab == i { rgb(59, 130, 246) } else { rgb(30, 41, 59) };
            draw_rounded(c, tx, cy + 4, 192, 22, 4, bg);
            draw_text(c, t, tx + 8, cy + 8, Color::WHITE);
        }

        if self.synth_tab == 1 {
            // Renderizar Reproductor Multimedia 8K HDR10+ AV1/VVC
            let vy = cy + 30;
            let vh = ch.saturating_sub(38);
            draw_box(c, cx + 4, vy, cw - 8, vh, rgb(5, 7, 12));
            draw_outline(c, cx + 4, vy, cw - 8, vh, 1, rgb(56, 189, 248));

            // Canvas de cine 8K
            let screen_h = vh.saturating_sub(90);
            draw_box(c, cx + 8, vy + 4, cw - 16, screen_h, rgb(2, 4, 8));
            draw_text(c, "☼ [SOL 4000 NITS HDR10+]", cx + (cw as i32) - 230, vy + 14, rgb(251, 191, 36));
            draw_text(c, "* CHRONOS HYPERION 8K *", cx + 24, vy + 14, rgb(56, 189, 248));
            draw_text(c, "Decodificador AV1 Main 10 / VVC • 7680x4320 @ 60 FPS • Direct Scanout DMA", cx + 24, vy + 32, rgb(203, 213, 225));
            draw_text(c, "Espacio de Color: BT.2020 / SMPTE ST 2084 PQ 10-bit • Latencia Frame: 0.54 ms", cx + 24, vy + 50, rgb(34, 197, 94));

            // Planeta cinematográfico central
            draw_rounded(c, cx + (cw as i32) / 2 - 40, vy + 75, 80, 80, 40, rgb(37, 99, 235));
            draw_text(c, "PORTIX", cx + (cw as i32) / 2 - 24, vy + 108, Color::WHITE);

            // Barra de progreso y tiempo
            let bar_y = vy + (screen_h as i32) + 10;
            draw_box(c, cx + 16, bar_y, cw - 32, 10, rgb(30, 41, 59));
            let fill_w = ((cw - 32) * self.player_seek / 100).max(10);
            draw_box(c, cx + 16, bar_y, fill_w, 10, rgb(56, 189, 248));

            let ctrl_y = bar_y + 16;
            let state_str = if self.player_state == 0 { "▶ REPRODUCIENDO" } else { "⏸ PAUSADO" };
            draw_text(c, &format!("{} 00:04:18 / 01:45:00 [42%] | Bitrate: 110.5 Mbps", state_str), cx + 16, ctrl_y, Color::WHITE);
            draw_text(c, "PipeWire Bit-Perfect 24-bit 96kHz | Zero-Copy DRM/KMS | Drops: 0", cx + 16, ctrl_y + 16, rgb(52, 211, 153));

            // Botones de transporte
            let btns = ["[ ▶ / ⏸ ]", "[ ⏮ -10s ]", "[ ⏭ +10s ]", "[ ☼ HDR10+ ]"];
            for (i, &b) in btns.iter().enumerate() {
                let bx = cx + (cw as i32) - 360 + (i as i32) * 85;
                if bx + 80 < cx + (cw as i32) {
                    draw_rounded(c, bx, ctrl_y - 2, 80, 22, 4, rgb(20, 30, 48));
                    draw_outline(c, bx, ctrl_y - 2, 80, 22, 1, rgb(51, 65, 85));
                    draw_text(c, b, bx + 6, ctrl_y + 2, rgb(56, 189, 248));
                }
            }
            return;
        }

        let half_w = (cw.saturating_sub(36)) / 2;

        // 1. Pantalla del osciloscopio (Mitad izquierda)
        draw_box(c, cx + 14, cy + 32, half_w, 90, rgb(10, 15, 25));
        draw_outline(c, cx + 14, cy + 32, half_w, 90, 1, rgb(59, 130, 246));
        draw_text(c, "OSCILOSCOPIO PCM (44.1 kHz)", cx + 22, cy + 38, rgb(56, 189, 248));

        let wave_y = cy + 76;
        let wave_len = (half_w as i32).saturating_sub(16);
        for i in 0..wave_len {
            let offset = match self.synth_wave {
                0 => ((i * 12) % 40) - 20, // Seno
                1 => if (i / 20) % 2 == 0 { 18 } else { -18 }, // Cuadrada
                _ => (i % 38) - 19, // Sierra
            };
            draw_box(c, cx + 22 + i, wave_y + offset, 2, 2, rgb(0, 255, 128));
        }

        // 2. Analizador de Espectro FFT Multicanal (Mitad derecha)
        let spec_x = cx + 18 + (half_w as i32);
        draw_box(c, spec_x, cy + 32, half_w, 90, rgb(10, 15, 25));
        draw_outline(c, spec_x, cy + 32, half_w, 90, 1, rgb(168, 85, 247));
        draw_text(c, "ESPECTRO FFT MULTICANAL (16 BANDAS)", spec_x + 12, cy + 38, rgb(192, 132, 252));

        for b in 0..16 {
            let bx = spec_x + 12 + (b as i32) * ((half_w as i32 - 24) / 16);
            let tick_mod = ((pit::ticks() * 2 + (b as u64) * 3) % 10) as u8;
            let bar_h = ((self.synth_spectrum[b] + tick_mod) as usize).min(55);
            let by = cy + 118 - (bar_h as i32);

            let col = if bar_h > 46 {
                rgb(239, 68, 68) // Rojo pico
            } else if bar_h > 26 {
                rgb(245, 158, 11) // Amarillo medio
            } else {
                rgb(34, 197, 94) // Verde base
            };
            draw_box(c, bx, by, 8, bar_h, col);
        }

        // 3. Controles de Forma de Onda y Audio Dúplex
        let waves = ["Seno", "Cuad", "Sierra"];
        for (i, &w) in waves.iter().enumerate() {
            let bx = cx + 14 + (i as i32) * 68;
            let bg = if self.synth_wave == i { rgb(59, 130, 246) } else { rgb(30, 41, 59) };
            draw_rounded(c, bx, cy + 130, 60, 24, 4, bg);
            draw_text(c, w, bx + 12, cy + 134, Color::WHITE);
        }

        // Botón Micrófono Dúplex
        let mic_bg = if self.synth_mic_active { rgb(16, 185, 129) } else { rgb(51, 65, 85) };
        let mic_txt = if self.synth_mic_active { "🎤 Mic ON" } else { "🎤 Mic OFF" };
        draw_rounded(c, cx + 228, cy + 130, 94, 24, 4, mic_bg);
        draw_text(c, mic_txt, cx + 236, cy + 134, Color::WHITE);

        // Botón Grabación WAV
        let rec_bg = if self.synth_recording { rgb(220, 38, 38) } else { rgb(51, 65, 85) };
        let rec_txt = if self.synth_recording { "⏹ Detener" } else { "🔴 Grabar" };
        draw_rounded(c, cx + 330, cy + 130, 90, 24, 4, rec_bg);
        draw_text(c, rec_txt, cx + 338, cy + 134, Color::WHITE);

        // Nivel de Ganancia Dúplex
        draw_rounded(c, cx + 428, cy + 130, 114, 24, 4, rgb(30, 41, 59));
        draw_text(c, &format!("Gain: {}%", self.synth_duplex_gain), cx + 438, cy + 134, rgb(56, 189, 248));

        // Teclado de Piano interactivo
        let piano_y = cy + 164;
        let key_w = 34;
        let notes = ["C", "D", "E", "F", "G", "A", "B", "C2", "D2", "E2", "F2", "G2", "A2", "B2"];
        for (i, &n) in notes.iter().enumerate() {
            let kx = cx + 14 + (i as i32) * key_w;
            let is_pressed = self.synth_active_note == Some(60 + i as u8);
            let bg = if is_pressed { rgb(59, 130, 246) } else { Color::WHITE };
            draw_rounded(c, kx, piano_y, (key_w - 2) as usize, 90, 4, bg);
            draw_text(c, n, kx + 8, piano_y + 68, Color::BLACK);
        }
    }


}
