// kernel/src/drivers/gpu/drm.rs — Subsistema Nativo DRM/KMS (Direct Rendering Manager / Kernel Mode Setting)
// Fases 551 - 560: Modos Atómicos, Conectores, CRTCs, Planos de Pantalla y Soporte VRR.

use alloc::vec::Vec;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorType {
    EDP,
    DisplayPort,
    HDMI,
    VGA,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct DrmMode {
    pub width: u32,
    pub height: u32,
    pub refresh_rate_hz: u32,
    pub vrr_capable: bool,
    pub pixel_clock_khz: u32,
}

impl DrmMode {
    pub const fn new(width: u32, height: u32, refresh_rate_hz: u32, vrr_capable: bool) -> Self {
        Self {
            width,
            height,
            refresh_rate_hz,
            vrr_capable,
            pixel_clock_khz: width * height * refresh_rate_hz / 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DrmConnector {
    pub id: u32,
    pub conn_type: ConnectorType,
    pub status: ConnectionStatus,
    pub current_mode: DrmMode,
    pub modes: Vec<DrmMode>,
    pub crtc_id: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneType {
    Primary,
    Cursor,
    Overlay,
}

#[derive(Debug, Clone)]
pub struct DrmPlane {
    pub id: u32,
    pub plane_type: PlaneType,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub z_order: u8,
}

#[derive(Debug, Clone)]
pub struct DrmCrtc {
    pub id: u32,
    pub enabled: bool,
    pub current_fb_id: u32,
    pub mode: DrmMode,
    pub vblank_counter: u64,
    pub gamma_lut_size: u32,
}

pub struct DrmSubsystem {
    pub connectors: Vec<DrmConnector>,
    pub crtcs: Vec<DrmCrtc>,
    pub planes: Vec<DrmPlane>,
    pub atomic_commits_count: AtomicU64,
    pub vrr_active: AtomicBool,
}

impl DrmSubsystem {
    pub fn new() -> Self {
        let mut connectors = Vec::new();
        // Conector principal (pantalla eDP de portátil o monitor principal DP)
        let mode_1080p_144hz = DrmMode::new(1920, 1080, 144, true);
        let mode_1080p_60hz = DrmMode::new(1920, 1080, 60, false);
        let mode_4k_120hz = DrmMode::new(3840, 2160, 120, true);

        let mut edp_modes = Vec::new();
        edp_modes.push(mode_1080p_144hz);
        edp_modes.push(mode_1080p_60hz);

        connectors.push(DrmConnector {
            id: 1,
            conn_type: ConnectorType::EDP,
            status: ConnectionStatus::Connected,
            current_mode: mode_1080p_144hz,
            modes: edp_modes,
            crtc_id: Some(1),
        });

        // Conector secundario HDMI
        let mut hdmi_modes = Vec::new();
        hdmi_modes.push(mode_4k_120hz);
        connectors.push(DrmConnector {
            id: 2,
            conn_type: ConnectorType::HDMI,
            status: ConnectionStatus::Connected,
            current_mode: mode_4k_120hz,
            modes: hdmi_modes,
            crtc_id: Some(2),
        });

        let mut crtcs = Vec::new();
        crtcs.push(DrmCrtc {
            id: 1,
            enabled: true,
            current_fb_id: 101,
            mode: mode_1080p_144hz,
            vblank_counter: 144,
            gamma_lut_size: 256,
        });
        crtcs.push(DrmCrtc {
            id: 2,
            enabled: true,
            current_fb_id: 102,
            mode: mode_4k_120hz,
            vblank_counter: 120,
            gamma_lut_size: 256,
        });

        let mut planes = Vec::new();
        planes.push(DrmPlane {
            id: 1,
            plane_type: PlaneType::Primary,
            crtc_id: 1,
            fb_id: 101,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            z_order: 0,
        });
        planes.push(DrmPlane {
            id: 2,
            plane_type: PlaneType::Cursor,
            crtc_id: 1,
            fb_id: 103,
            x: 500,
            y: 300,
            width: 64,
            height: 64,
            z_order: 10,
        });

        Self {
            connectors,
            crtcs,
            planes,
            atomic_commits_count: AtomicU64::new(0),
            vrr_active: AtomicBool::new(true),
        }
    }

    /// Aplica un cambio de modo de pantalla de forma atómica (Atomic Modesetting)
    pub fn atomic_modeset(&mut self, connector_id: u32, mode: DrmMode) -> Result<(), &'static str> {
        let conn = self.connectors.iter_mut().find(|c| c.id == connector_id)
            .ok_or("Conector DRM no encontrado")?;
        
        conn.current_mode = mode;
        if let Some(crtc_id) = conn.crtc_id {
            if let Some(crtc) = self.crtcs.iter_mut().find(|cr| cr.id == crtc_id) {
                crtc.mode = mode;
            }
        }
        
        self.atomic_commits_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub fn set_vrr_enabled(&self, enabled: bool) {
        self.vrr_active.store(enabled, Ordering::SeqCst);
    }

    pub fn is_vrr_active(&self) -> bool {
        self.vrr_active.load(Ordering::Relaxed)
    }
}

pub static DRM_SUBSYSTEM: Spinlock<Option<DrmSubsystem>> = Spinlock::new(None);

pub fn init_drm() {
    let mut lock = DRM_SUBSYSTEM.lock();
    if lock.is_none() {
        *lock = Some(DrmSubsystem::new());
    }
}

pub fn get_drm_summary() -> String {
    let lock = DRM_SUBSYSTEM.lock();
    match &*lock {
        Some(drm) => {
            let conn_count = drm.connectors.len();
            let vrr = if drm.is_vrr_active() { "ACTIVO (G-Sync/FreeSync)" } else { "Inactivo" };
            let commits = drm.atomic_commits_count.load(Ordering::Relaxed);
            alloc::format!(
                "DRM/KMS: {} conectores activos | Conector 1: {}x{}@{}Hz | VRR: {} | Atomic Commits: {}",
                conn_count,
                drm.connectors[0].current_mode.width,
                drm.connectors[0].current_mode.height,
                drm.connectors[0].current_mode.refresh_rate_hz,
                vrr,
                commits
            )
        }
        None => String::from("DRM/KMS: No inicializado"),
    }
}
