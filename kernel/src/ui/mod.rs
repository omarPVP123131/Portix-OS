// ui/mod.rs — Componentes de interfaz de usuario

pub mod chrome;
pub mod exception;
pub mod tabs;

pub use chrome::{section_label, draw_chrome};
pub use exception::draw_exception;
pub use tabs::{draw_system_tab, draw_terminal_tab, draw_devices_tab};
pub use tabs::terminal::terminal_hist_geometry;

/// Ancho de la barra de scroll en píxeles
pub const SCROLLBAR_W: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    System   = 0,
    Terminal = 1,
    Devices  = 2,
}
