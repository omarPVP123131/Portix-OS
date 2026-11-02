//! Portix-Settings: Centro de Control y Ajustes del Sistema.
//!
//! Permite conmutar temas (Oscuro/Claro/OLED), colores de acento,
//! resolución, tasa de refresco (60 a 240 Hz), escalado HiDPI,
//! calibración de gestos de trackpad y perfiles de energía ACPI.

use alloc::string::String;
use crate::arch::Spinlock;
use crate::ui::portix_ui::theme::{ThemeMode, AccentColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    BatterySaver,
    Balanced,
    UltraPerformanceTurbo,
}

pub struct PortixSettingsApp {
    pub theme: ThemeMode,
    pub accent: AccentColor,
    pub display_resolution: (u32, u32),
    pub refresh_rate_hz: u16,
    pub hidpi_scale_pct: u16, // 100, 125, 150, 200
    pub natural_scrolling: bool,
    pub tap_to_click: bool,
    pub trackpad_sensitivity: u8, // 1 a 10
    pub palm_rejection_enabled: bool,
    pub power_profile: PowerProfile,
}

impl PortixSettingsApp {
    pub fn new() -> Self {
        Self {
            theme: ThemeMode::Dark,
            accent: AccentColor::BlueNeon,
            display_resolution: (2560, 1440),
            refresh_rate_hz: 144,
            hidpi_scale_pct: 125,
            natural_scrolling: true,
            tap_to_click: true,
            trackpad_sensitivity: 7,
            palm_rejection_enabled: true,
            power_profile: PowerProfile::UltraPerformanceTurbo,
        }
    }

    pub fn set_theme(&mut self, mode: ThemeMode) {
        self.theme = mode;
    }

    pub fn set_accent(&mut self, accent: AccentColor) {
        self.accent = accent;
    }

    pub fn set_display_mode(&mut self, width: u32, height: u32, hz: u16, scale: u16) {
        self.display_resolution = (width, height);
        self.refresh_rate_hz = hz;
        self.hidpi_scale_pct = scale;
    }

    pub fn set_power_profile(&mut self, profile: PowerProfile) {
        self.power_profile = profile;
    }

    pub fn get_summary(&self) -> String {
        let theme_str = match self.theme {
            ThemeMode::Dark => "Oscuro Moderno",
            ThemeMode::Light => "Claro Minimalista",
            ThemeMode::OledBlack => "OLED Puro (Cero Energia)",
            ThemeMode::HighContrast => "Alto Contraste (Accesibilidad)",
        };
        let accent_str = match self.accent {
            AccentColor::BlueNeon => "Azul Neon",
            AccentColor::Emerald => "Verde Esmeralda",
            AccentColor::Violet => "Violeta Electrico",
            AccentColor::Amber => "Oro Ambar",
            AccentColor::Crimson => "Rojo Carmesi",
        };
        let pwr_str = match self.power_profile {
            PowerProfile::BatterySaver => "Ahorro de Bateria",
            PowerProfile::Balanced => "Equilibrado Inteligente",
            PowerProfile::UltraPerformanceTurbo => "Ultra-Rendimiento Turbo (Cero Latencia)",
        };
        alloc::format!(
            "Portix-Settings | Tema: {} ({}) | Pantalla: {}x{} @ {}Hz (HiDPI {}%) | Trackpad: Natural={}, Tap={}, Sens={}/10 | Energia: {}",
            theme_str,
            accent_str,
            self.display_resolution.0,
            self.display_resolution.1,
            self.refresh_rate_hz,
            self.hidpi_scale_pct,
            self.natural_scrolling,
            self.tap_to_click,
            self.trackpad_sensitivity,
            pwr_str
        )
    }
}

pub static GLOBAL_PORTIX_SETTINGS: Spinlock<Option<PortixSettingsApp>> = Spinlock::new(None);

pub fn init_portix_settings() {
    let mut lock = GLOBAL_PORTIX_SETTINGS.lock();
    if lock.is_none() {
        *lock = Some(PortixSettingsApp::new());
    }
}

pub fn get_settings_summary() -> String {
    let mut lock = GLOBAL_PORTIX_SETTINGS.lock();
    if lock.is_none() {
        *lock = Some(PortixSettingsApp::new());
    }
    lock.as_ref().map(|s| s.get_summary()).unwrap_or_else(|| String::from("Portix-Settings no inicializado"))
}
