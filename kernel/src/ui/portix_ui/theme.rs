// kernel/src/ui/portix_ui/theme.rs — Sistema de Diseño y Temas Dinámicos de Portix-UI
// Fases 651 - 700: Modos Claro/Oscuro, Colores de Acento, Esquinas Redondeadas y Efectos Acrílicos.

use core::sync::atomic::{AtomicU32, Ordering};
use crate::arch::spinlock::Spinlock;
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    OledBlack,
    HighContrast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentColor {
    BlueNeon,
    Emerald,
    Violet,
    Amber,
    Crimson,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeConfig {
    pub mode: ThemeMode,
    pub accent_type: AccentColor,
    pub background_color: u32,
    pub surface_color: u32,
    pub surface_elevated: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub accent_color: u32,
    pub border_color: u32,
    pub border_radius_px: u32,
    pub acrylic_blur_radius: u32,
}

impl ThemeConfig {
    pub const fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            accent_type: AccentColor::BlueNeon,
            background_color: 0xFF121417,
            surface_color: 0xFF1E2228,
            surface_elevated: 0xFF2A303A,
            text_primary: 0xFFF0F2F5,
            text_secondary: 0xFF9BA3AF,
            accent_color: 0xFF3B82F6, // Blue Neon
            border_color: 0xFF374151,
            border_radius_px: 10,
            acrylic_blur_radius: 24,
        }
    }

    pub const fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            accent_type: AccentColor::BlueNeon,
            background_color: 0xFFF8FAFC,
            surface_color: 0xFFFFFFFF,
            surface_elevated: 0xFFF1F5F9,
            text_primary: 0xFF0F172A,
            text_secondary: 0xFF64748B,
            accent_color: 0xFF2563EB,
            border_color: 0xFFE2E8F0,
            border_radius_px: 10,
            acrylic_blur_radius: 20,
        }
    }
}

pub static CURRENT_THEME: Spinlock<ThemeConfig> = Spinlock::new(ThemeConfig::dark());
pub static THEME_SWITCH_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn set_theme_mode(mode: ThemeMode) {
    let mut theme = CURRENT_THEME.lock();
    let accent = theme.accent_type;
    *theme = match mode {
        ThemeMode::Dark => ThemeConfig::dark(),
        ThemeMode::Light => ThemeConfig::light(),
        ThemeMode::OledBlack => {
            let mut t = ThemeConfig::dark();
            t.background_color = 0xFF000000;
            t.surface_color = 0xFF0A0A0A;
            t.mode = ThemeMode::OledBlack;
            t
        }
        ThemeMode::HighContrast => {
            let mut t = ThemeConfig::dark();
            t.background_color = 0xFF000000;
            t.surface_color = 0xFF111111;
            t.text_primary = 0xFFFFFFFF;
            t.accent_color = 0xFFFFFF00; // Amarillo de alto contraste
            t.border_color = 0xFFFFFFFF;
            t.mode = ThemeMode::HighContrast;
            t
        }
    };
    theme.accent_type = accent;
    THEME_SWITCH_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn set_accent(accent: AccentColor) {
    let mut theme = CURRENT_THEME.lock();
    theme.accent_type = accent;
    theme.accent_color = match accent {
        AccentColor::BlueNeon => 0xFF3B82F6,
        AccentColor::Emerald => 0xFF10B981,
        AccentColor::Violet => 0xFF8B5CF6,
        AccentColor::Amber => 0xFFF59E0B,
        AccentColor::Crimson => 0xFFEF4444,
    };
}

pub fn get_theme_summary() -> String {
    let theme = CURRENT_THEME.lock();
    let mode_str = match theme.mode {
        ThemeMode::Dark => "Oscuro",
        ThemeMode::Light => "Claro",
        ThemeMode::OledBlack => "OLED Negro Absoluto",
        ThemeMode::HighContrast => "Alto Contraste",
    };
    let accent_str = match theme.accent_type {
        AccentColor::BlueNeon => "Azul Neón",
        AccentColor::Emerald => "Esmeralda",
        AccentColor::Violet => "Violeta",
        AccentColor::Amber => "Ámbar",
        AccentColor::Crimson => "Carmesí",
    };
    alloc::format!(
        "Tema Activo: {} | Acento: {} | Radio Esquinas: {}px | Desenfoque Acrílico: {}px",
        mode_str, accent_str, theme.border_radius_px, theme.acrylic_blur_radius
    )
}
