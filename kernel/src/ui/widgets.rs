// kernel/src/ui/widgets.rs — Toolkit Declarativo de Widgets Adwaita-Rust y Portapapeles Global
// Botones con animación de foco, campos de texto reactivos, scrollbars y portapapeles /dev/clipboard
#![allow(dead_code)]

use crate::arch::spinlock::Spinlock;

// ── Portapapeles Global del Sistema (/dev/clipboard) ─────────────────────────

pub struct GlobalClipboard {
    pub content: &'static str,
    pub copy_count: u64,
    pub paste_count: u64,
}

impl GlobalClipboard {
    pub const fn new() -> Self {
        Self {
            content: "Portix Universal System",
            copy_count: 1,
            paste_count: 0,
        }
    }

    pub fn copy(&mut self, text: &'static str) {
        self.content = text;
        self.copy_count += 1;
    }

    pub fn paste(&mut self) -> &'static str {
        self.paste_count += 1;
        self.content
    }

    pub fn clear(&mut self) {
        self.content = "";
    }
}

pub static CLIPBOARD: Spinlock<GlobalClipboard> = Spinlock::new(GlobalClipboard::new());

// ── Estados y Widgets Reactivos Adwaita-Rust ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetState {
    Normal,
    Hover,
    Pressed,
    Focused,
    Disabled,
}

/// Botón interactivo con esquinas redondeadas y animación de sombra
#[derive(Debug, Clone, Copy)]
pub struct AdwaitaButton {
    pub label: &'static str,
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub border_radius: usize,
    pub state: WidgetState,
    pub click_count: u32,
}

impl AdwaitaButton {
    pub const fn new(label: &'static str, x: i32, y: i32, width: usize, height: usize) -> Self {
        Self {
            label,
            x,
            y,
            width,
            height,
            border_radius: 6,
            state: WidgetState::Normal,
            click_count: 0,
        }
    }

    pub fn handle_mouse(&mut self, mx: i32, my: i32, pressed: bool) -> bool {
        let inside = mx >= self.x && mx <= (self.x + self.width as i32) &&
                     my >= self.y && my <= (self.y + self.height as i32);

        if inside {
            if pressed {
                self.state = WidgetState::Pressed;
                self.click_count += 1;
                return true;
            } else {
                self.state = WidgetState::Hover;
            }
        } else {
            self.state = WidgetState::Normal;
        }
        false
    }
}

/// Campo de texto editable (Entry/TextField) con cursor y portapapeles
#[derive(Debug, Clone, Copy)]
pub struct AdwaitaEntry {
    pub placeholder: &'static str,
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub cursor_pos: usize,
    pub is_focused: bool,
}

impl AdwaitaEntry {
    pub const fn new(placeholder: &'static str, x: i32, y: i32, width: usize, height: usize) -> Self {
        Self {
            placeholder,
            x,
            y,
            width,
            height,
            cursor_pos: 0,
            is_focused: false,
        }
    }
}

/// Barra de desplazamiento interactiva
#[derive(Debug, Clone, Copy)]
pub struct AdwaitaScrollbar {
    pub x: i32,
    pub y: i32,
    pub length: usize,
    pub is_vertical: bool,
    pub thumb_pos: f32, // 0.0 a 1.0
    pub thumb_size: f32, // Proporción visible
    pub is_dragging: bool,
}

impl AdwaitaScrollbar {
    pub const fn new(x: i32, y: i32, length: usize, is_vertical: bool) -> Self {
        Self {
            x,
            y,
            length,
            is_vertical,
            thumb_pos: 0.0,
            thumb_size: 0.25,
            is_dragging: false,
        }
    }

    pub fn set_scroll_ratio(&mut self, ratio: f32) {
        self.thumb_pos = ratio.clamp(0.0, 1.0 - self.thumb_size);
    }
}

/// Diálogo Modal de Confirmación
#[derive(Debug, Clone, Copy)]
pub struct AdwaitaModalDialog {
    pub title: &'static str,
    pub message: &'static str,
    pub is_open: bool,
    pub confirmed: bool,
}

impl AdwaitaModalDialog {
    pub const fn new(title: &'static str, message: &'static str) -> Self {
        Self {
            title,
            message,
            is_open: false,
            confirmed: false,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.confirmed = false;
    }

    pub fn confirm(&mut self) {
        self.confirmed = true;
        self.is_open = false;
    }

    pub fn cancel(&mut self) {
        self.confirmed = false;
        self.is_open = false;
    }
}

// ── Gestor Global de Widgets ─────────────────────────────────────────────────

pub struct WidgetManager {
    pub sample_button: AdwaitaButton,
    pub sample_entry: AdwaitaEntry,
    pub sample_dialog: AdwaitaModalDialog,
    pub is_initialized: bool,
}

impl WidgetManager {
    pub const fn new() -> Self {
        Self {
            sample_button: AdwaitaButton::new("Aceptar", 100, 100, 90, 32),
            sample_entry: AdwaitaEntry::new("Escriba un comando...", 100, 150, 240, 32),
            sample_dialog: AdwaitaModalDialog::new("Confirmacion", "Desea aplicar los cambios?"),
            is_initialized: true,
        }
    }
}

pub static WIDGET_MGR: Spinlock<WidgetManager> = Spinlock::new(WidgetManager::new());

/// Inicialización del gestor de widgets
pub fn init() {
    let mut mgr = WIDGET_MGR.lock();
    mgr.is_initialized = true;
    CLIPBOARD.lock().copy("Portix Universal System");
}
