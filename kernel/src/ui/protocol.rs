// kernel/src/ui/protocol.rs — Wayland / X11-Style UI Compositor & Event Protocol
// Provides client-server separation, surface abstraction, and event dispatching.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::graphics::driver::framebuffer::Color;

/// Identificador único de superficie o ventana en el compositor
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SurfaceId(pub u32);

/// Tipos de eventos del compositor inspirados en el protocolo Wayland wl_keyboard / wl_pointer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorEvent {
    KeyDown { key_code: u8, shift: bool, ctrl: bool },
    KeyUp { key_code: u8 },
    Char(u8),
    MouseDown { x: i32, y: i32, button: u8 }, // 1: Izq, 2: Der, 3: Medio
    MouseUp { x: i32, y: i32, button: u8 },
    MouseMove { x: i32, y: i32 },
    MouseScroll { delta: i32 },
    WindowResize { w: usize, h: usize },
    WindowFocus(bool),
    WindowClose,
}

/// Cola de eventos de entrada por superficie (Event Queue)
pub struct EventQueue {
    events: Vec<CompositorEvent>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, ev: CompositorEvent) {
        if self.events.len() < 128 {
            self.events.push(ev);
        }
    }

    pub fn pop(&mut self) -> Option<CompositorEvent> {
        if self.events.is_empty() {
            None
        } else {
            Some(self.events.remove(0))
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Búfer de flujo de texto y terminal virtual para clientes CLI (PTY Abstraction)
pub struct PtyStream {
    pub lines: Vec<String>,
    pub max_lines: usize,
    pub cursor_col: usize,
    pub prompt: &'static str,
    pub input_buffer: String,
    pub history: Vec<String>,
    pub hist_index: usize,
    pub scroll_offset: usize,
}

impl PtyStream {
    pub fn new(max_lines: usize, prompt: &'static str) -> Self {
        Self {
            lines: Vec::new(),
            max_lines,
            cursor_col: 0,
            prompt,
            input_buffer: String::new(),
            history: Vec::new(),
            hist_index: 0,
            scroll_offset: 0,
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for line in s.lines() {
            self.push_line(String::from(line));
        }
    }

    pub fn write_ansi(&mut self, s: &str) {
        if s.contains("\x1b[2J") {
            self.clear();
        }
        for line in s.lines() {
            if !line.contains("\x1b[2J") {
                self.push_line(String::from(line));
            }
        }
    }

    pub fn push_line(&mut self, line: String) {
        if self.lines.len() >= self.max_lines {
            self.lines.remove(0);
        }
        self.lines.push(line);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }
}

/// Protocolo de Superficie Gráfica (wl_surface)
pub struct WindowSurface {
    pub id: SurfaceId,
    pub width: usize,
    pub height: usize,
    pub event_queue: EventQueue,
    pub pty: Option<PtyStream>,
    pub background: Color,
    pub dirty: bool,
}

impl WindowSurface {
    pub fn new(id: SurfaceId, width: usize, height: usize, bg: Color) -> Self {
        Self {
            id,
            width,
            height,
            event_queue: EventQueue::new(),
            pty: None,
            background: bg,
            dirty: true,
        }
    }

    pub fn with_pty(mut self, max_lines: usize, prompt: &'static str) -> Self {
        self.pty = Some(PtyStream::new(max_lines, prompt));
        self
    }
}

/// Buffer de Memoria Compartida para Compositor Wayland (wl_shm_pool / wl_buffer)
pub struct ShmBuffer {
    pub surface_id: SurfaceId,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub data: Vec<u32>, // Pixeles ARGB de 32 bits
}

impl ShmBuffer {
    pub fn new(surface_id: SurfaceId, width: usize, height: usize) -> Self {
        let size = width * height;
        let mut data = Vec::with_capacity(size);
        data.resize(size, 0xFF000000); // Negro opaco
        Self {
            surface_id,
            width,
            height,
            stride: width * 4,
            data,
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.data[y * self.width + x] = color;
        }
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.data[y * self.width + x]
        } else {
            0
        }
    }
}

/// Mezcla alfa de dos colores (Alpha Blending) para sombras suaves y ventanas translúcidas
pub fn blend_color_alpha(bg: Color, fg: Color, alpha: u8) -> Color {
    fg.blend(bg, alpha)
}
