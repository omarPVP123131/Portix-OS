// kernel/src/compat/i18n.rs — Internationalization (i18n) and Universal Accessibility (a11y)
// Provides system locale translations and text-to-speech / high-contrast primitives.

#![allow(dead_code)]

use alloc::string::String;
use crate::arch::Spinlock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemLanguage {
    Spanish,  // es-ES
    English,  // en-US
    Japanese, // ja-JP
    German,   // de-DE
}

pub struct I18nEngine {
    pub current_lang: SystemLanguage,
}

impl I18nEngine {
    pub const fn new() -> Self {
        I18nEngine {
            current_lang: SystemLanguage::Spanish,
        }
    }

    pub fn set_language(&mut self, lang: SystemLanguage) {
        self.current_lang = lang;
    }

    pub fn cycle_language(&mut self) -> SystemLanguage {
        self.current_lang = match self.current_lang {
            SystemLanguage::Spanish => SystemLanguage::English,
            SystemLanguage::English => SystemLanguage::Japanese,
            SystemLanguage::Japanese => SystemLanguage::German,
            SystemLanguage::German => SystemLanguage::Spanish,
        };
        self.current_lang
    }

    pub fn lang_code(&self) -> &'static str {
        match self.current_lang {
            SystemLanguage::Spanish => "ES",
            SystemLanguage::English => "EN",
            SystemLanguage::Japanese => "JA",
            SystemLanguage::German => "DE",
        }
    }

    pub fn translate<'a>(&self, key: &str) -> &'a str {
        match self.current_lang {
            SystemLanguage::Spanish => match key {
                "welcome" => "Bienvenido a Portix OS v6.0 Chronos",
                "ready" => "Sistema operativo soberano listo para produccion",
                "storage" => "Almacenamiento VFS, NVMe y CoW montados correctamente",
                "network" => "Pila de red IPv6, SMB3, WireGuard y XDP activa",
                "security" => "Raiz de confianza TPM 2.0 y Secure Boot verificados al 100%",
                "governor" => "Regulador de frecuencia y planificador CFS afinados",
                "workspaces" => "Escritorios",
                "silicon" => "Silicio",
                "bench" => "Rendimiento",
                "mesh" => "Malla P2P",
                "apps" => "Aplicaciones",
                _ => "Cadena no encontrada",
            },
            SystemLanguage::English => match key {
                "welcome" => "Welcome to Portix OS v6.0 Chronos",
                "ready" => "Sovereign operating system ready for production",
                "storage" => "VFS, NVMe, and CoW storage mounted successfully",
                "network" => "IPv6, SMB3, WireGuard, and XDP network stack active",
                "security" => "TPM 2.0 root of trust and Secure Boot 100% verified",
                "governor" => "Frequency governor and CFS scheduler optimized",
                "workspaces" => "Workspaces",
                "silicon" => "Silicon",
                "bench" => "Benchmarks",
                "mesh" => "P2P Mesh",
                "apps" => "Applications",
                _ => "String not found",
            },
            SystemLanguage::Japanese => match key {
                "welcome" => "Portix OS v6.0 クロノスへようこそ",
                "ready" => "独立したOSの本番環境準備完了",
                "storage" => "VFS、NVMeおよびCoWストレージが正常にマウントされました",
                "network" => "IPv6、SMB3、WireGuardおよびXDPネットワーク有効",
                "security" => "TPM 2.0ルートオブトラストおよびセキュアブート100%検証済",
                "governor" => "周波数ガバナーとCFSスケジューラ最適化完了",
                "workspaces" => "ワークスペース",
                "silicon" => "シリコン",
                "bench" => "ベンチマーク",
                "mesh" => "P2Pメッシュ",
                "apps" => "アプリ",
                _ => "文字列が見つかりません",
            },
            SystemLanguage::German => match key {
                "welcome" => "Willkommen bei Portix OS v6.0 Chronos",
                "ready" => "Souveränes Betriebssystem betriebsbereit",
                "storage" => "VFS, NVMe und CoW erfolgreich eingehängt",
                "network" => "IPv6, SMB3, WireGuard und XDP aktiv",
                "security" => "TPM 2.0 Vertrauensanker und Secure Boot zu 100% verifiziert",
                "governor" => "Frequenzregler und CFS-Planer abgestimmt",
                "workspaces" => "Arbeitsbereiche",
                "silicon" => "Silizium",
                "bench" => "Leistung",
                "mesh" => "P2P-Netz",
                "apps" => "Anwendungen",
                _ => "Zeichenkette nicht gefunden",
            },
        }
    }
}

pub struct A11ySubsystem {
    pub high_contrast: bool,
    pub font_scale_percent: u16, // 100, 150, 200
    pub screen_reader_active: bool,
    pub sticky_keys: bool,
    pub last_spoken_phrase: String,
}

impl A11ySubsystem {
    pub const fn new() -> Self {
        A11ySubsystem {
            high_contrast: false,
            font_scale_percent: 100,
            screen_reader_active: false,
            sticky_keys: false,
            last_spoken_phrase: String::new(),
        }
    }

    pub fn toggle_high_contrast(&mut self) -> bool {
        self.high_contrast = !self.high_contrast;
        self.high_contrast
    }

    pub fn set_font_scale(&mut self, scale: u16) {
        if scale >= 100 && scale <= 250 {
            self.font_scale_percent = scale;
        }
    }

    pub fn speak_text(&mut self, phrase: &str) {
        self.last_spoken_phrase = String::from(phrase);
        crate::drivers::serial::write_str("[TTS-A11Y] ");
        crate::drivers::serial::write_str(phrase);
        crate::drivers::serial::write_str("\n");
    }
}

pub static GLOBAL_I18N: Spinlock<I18nEngine> = Spinlock::new(I18nEngine::new());
pub static GLOBAL_A11Y: Spinlock<A11ySubsystem> = Spinlock::new(A11ySubsystem::new());
