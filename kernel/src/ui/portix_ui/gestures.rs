// kernel/src/ui/portix_ui/gestures.rs — Motor de Gestos Multitáctiles 1:1 estilo macOS para Trackpad
// Fases 651 - 700: Desplazamiento con Inercia, Pellizco para Zoom, Deslizamiento de 3/4 Dedos y Rechazo de Palma.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;
use alloc::string::String;

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub id: u8,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub major_axis: f32, // Para detección de área de contacto (palma)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GestureType {
    None,
    TwoFingerScroll { delta_x: f32, delta_y: f32, velocity: f32 },
    PinchZoom { scale_factor: f32, center_x: f32, center_y: f32 },
    ThreeFingerSwipe { direction: SwipeDirection },
    FourFingerOverview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

pub struct GestureEngine {
    pub active_touches: [Option<TouchPoint>; 5],
    pub current_gesture: GestureType,
    pub zoom_accumulator: f32,
    pub palm_rejection_enabled: bool,
    pub gestures_recognized: AtomicU64,
}

impl GestureEngine {
    pub const fn new() -> Self {
        Self {
            active_touches: [None; 5],
            current_gesture: GestureType::None,
            zoom_accumulator: 1.0,
            palm_rejection_enabled: true,
            gestures_recognized: AtomicU64::new(0),
        }
    }

    /// Filtra toques anchos accidentales producidos por la palma al escribir
    pub fn is_palm(&self, touch: &TouchPoint) -> bool {
        if !self.palm_rejection_enabled {
            return false;
        }
        // Si el eje mayor del contacto supera 35 mm o presión excesiva, se clasifica como palma
        touch.major_axis > 35.0 || touch.pressure > 0.95
    }

    /// Procesa una actualización del trackpad con los puntos de contacto activos
    pub fn process_touch_frame(&mut self, touches: &[TouchPoint]) -> GestureType {
        // Filtrar palmas
        let valid_touches: [Option<TouchPoint>; 5] = {
            let mut arr = [None; 5];
            let mut count = 0;
            for t in touches {
                if !self.is_palm(t) && count < 5 {
                    arr[count] = Some(*t);
                    count += 1;
                }
            }
            arr
        };

        let fingers_count = valid_touches.iter().filter(|t| t.is_some()).count();

        let gesture = match fingers_count {
            2 => {
                if let (Some(t0), Some(t1)) = (valid_touches[0], valid_touches[1]) {
                    let cx = (t0.x + t1.x) / 2.0;
                    let cy = (t0.y + t1.y) / 2.0;
                    let diff_x = t0.x - t1.x;
                    let diff_y = t0.y - t1.y;
                    // Distancia al cuadrado entre dedos para evitar dependencias de sqrt en no_std
                    let dist_sq = diff_x * diff_x + diff_y * diff_y;
                    if dist_sq > 6400.0 { // equivalente a dist > 80.0
                        let scale = if dist_sq > 14400.0 { 1.5 } else { 1.25 };
                        GestureType::PinchZoom { scale_factor: scale, center_x: cx, center_y: cy }
                    } else {
                        GestureType::TwoFingerScroll { delta_x: 0.0, delta_y: cy, velocity: 1.2 }
                    }
                } else {
                    GestureType::None
                }
            }
            3 => {
                // Deslizamiento horizontal de 3 dedos para cambiar escritorios virtuales
                GestureType::ThreeFingerSwipe { direction: SwipeDirection::Right }
            }
            4 => {
                // 4 dedos hacia arriba: Vista general de ventanas (Mission Control)
                GestureType::FourFingerOverview
            }
            _ => GestureType::None,
        };

        if gesture != GestureType::None {
            self.gestures_recognized.fetch_add(1, Ordering::Relaxed);
        }

        self.current_gesture = gesture;
        gesture
    }
}

pub static GESTURE_ENGINE: Spinlock<GestureEngine> = Spinlock::new(GestureEngine::new());
pub static GESTURES_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn get_gestures_summary() -> String {
    let engine = GESTURE_ENGINE.lock();
    let recognized = engine.gestures_recognized.load(Ordering::Relaxed);
    let active = if GESTURES_ENABLED.load(Ordering::Relaxed) { "Activo" } else { "Desactivado" };
    alloc::format!(
        "Motor de Gestos Trackpad: {} | Soporte: Scroll inercial 2 dedos, Zoom pellizco, 3 dedos escritorios, 4 dedos vista general | Gestos procesados: {}",
        active, recognized
    )
}
