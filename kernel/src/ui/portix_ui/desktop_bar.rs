// kernel/src/ui/portix_ui/desktop_bar.rs — Barra de Tareas Modular Portix-Bar
// Fases 701 - 750: Menú de Inicio, Paginador de Escritorios, Bandeja del Sistema y Reloj.

use alloc::vec::Vec;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarPosition {
    Top,
    Bottom,
}

#[derive(Debug, Clone)]
pub struct SystemTrayIcon {
    pub name: String,
    pub tooltip: String,
    pub is_active: bool,
    pub value_percentage: Option<u8>,
}

#[derive(Debug)]
pub struct PortixBar {
    pub height_px: u32,
    pub position: BarPosition,
    pub active_desktop: AtomicU8,
    pub total_desktops: u8,
    pub is_menu_open: AtomicBool,
    pub tray_icons: Vec<SystemTrayIcon>,
}

impl PortixBar {
    pub fn new() -> Self {
        let mut tray = Vec::new();
        tray.push(SystemTrayIcon {
            name: String::from("wifi"),
            tooltip: String::from("Wi-Fi 6E Conectado (5.8 GHz)"),
            is_active: true,
            value_percentage: Some(92),
        });
        tray.push(SystemTrayIcon {
            name: String::from("bluetooth"),
            tooltip: String::from("Bluetooth 5.4 Activo"),
            is_active: true,
            value_percentage: None,
        });
        tray.push(SystemTrayIcon {
            name: String::from("battery"),
            tooltip: String::from("Batería: 88% (Restan 9h 15m)"),
            is_active: true,
            value_percentage: Some(88),
        });
        tray.push(SystemTrayIcon {
            name: String::from("volume"),
            tooltip: String::from("Volumen: 65% (Bit-Perfect PipeWire)"),
            is_active: true,
            value_percentage: Some(65),
        });

        Self {
            height_px: 36,
            position: BarPosition::Top,
            active_desktop: AtomicU8::new(1),
            total_desktops: 4,
            is_menu_open: AtomicBool::new(false),
            tray_icons: tray,
        }
    }

    pub fn switch_desktop(&self, desktop: u8) {
        if desktop >= 1 && desktop <= self.total_desktops {
            self.active_desktop.store(desktop, Ordering::SeqCst);
        }
    }

    pub fn toggle_menu(&self) -> bool {
        let current = self.is_menu_open.load(Ordering::Relaxed);
        self.is_menu_open.store(!current, Ordering::SeqCst);
        !current
    }
}

pub static PORTIX_BAR: Spinlock<Option<PortixBar>> = Spinlock::new(None);

pub fn init_desktop_bar() {
    let mut lock = PORTIX_BAR.lock();
    if lock.is_none() {
        *lock = Some(PortixBar::new());
    }
}

pub fn get_bar_summary() -> String {
    let lock = PORTIX_BAR.lock();
    match &*lock {
        Some(bar) => {
            let desk = bar.active_desktop.load(Ordering::Relaxed);
            let tray_count = bar.tray_icons.len();
            let pos = match bar.position {
                BarPosition::Top => "Superior",
                BarPosition::Bottom => "Inferior",
            };
            alloc::format!(
                "Portix-Bar: Barra {} ({}px) | Escritorio Activo: {}/{} | Iconos Bandeja: {}",
                pos, bar.height_px, desk, bar.total_desktops, tray_count
            )
        }
        None => String::from("Portix-Bar: No inicializada"),
    }
}
