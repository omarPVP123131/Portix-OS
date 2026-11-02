// kernel/src/arch/backlight.rs — Gestión de Pantalla y Brillo Dinámico (Backlight PWM)
// Implementa el control de retroiluminación PWM y temporizador de inactividad para Portix OS

#![allow(dead_code)]

use crate::arch::Spinlock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BacklightPowerMode {
    On,
    Dimmed,
    Standby,
    Off,
}

pub struct BacklightManager {
    pub brightness_pct: u8, // 0 - 100
    pub max_brightness_pwm: u16,
    pub current_pwm_duty: u16,
    pub power_mode: BacklightPowerMode,
    pub inactivity_seconds: u32,
    pub dim_timeout_seconds: u32,
    pub blank_timeout_seconds: u32,
    pub ambient_light_sensor_lux: u32,
    pub auto_brightness_enabled: bool,
}

impl BacklightManager {
    pub const fn new() -> Self {
        BacklightManager {
            brightness_pct: 80,
            max_brightness_pwm: 1000,
            current_pwm_duty: 800,
            power_mode: BacklightPowerMode::On,
            inactivity_seconds: 0,
            dim_timeout_seconds: 300,  // 5 min
            blank_timeout_seconds: 600, // 10 min
            ambient_light_sensor_lux: 450,
            auto_brightness_enabled: true,
        }
    }

    /// Asigna el nivel de brillo de 0 a 100%
    pub fn set_brightness(&mut self, pct: u8) {
        let clamped = if pct > 100 { 100 } else { pct };
        self.brightness_pct = clamped;
        self.current_pwm_duty = (clamped as u32 * self.max_brightness_pwm as u32 / 100) as u16;
        if clamped == 0 {
            self.power_mode = BacklightPowerMode::Off;
        } else {
            self.power_mode = BacklightPowerMode::On;
        }
    }

    /// Ticks del temporizador de inactividad de usuario
    pub fn update_inactivity(&mut self, delta_sec: u32) {
        self.inactivity_seconds += delta_sec;
        if self.inactivity_seconds >= self.blank_timeout_seconds {
            self.power_mode = BacklightPowerMode::Off;
            self.current_pwm_duty = 0;
        } else if self.inactivity_seconds >= self.dim_timeout_seconds {
            self.power_mode = BacklightPowerMode::Dimmed;
            self.current_pwm_duty = self.max_brightness_pwm / 5; // 20% tenue
        } else {
            self.power_mode = BacklightPowerMode::On;
            self.current_pwm_duty = (self.brightness_pct as u32 * self.max_brightness_pwm as u32 / 100) as u16;
        }
    }

    /// Restablece el contador de inactividad cuando hay eventos de ratón o teclado
    pub fn reset_inactivity(&mut self) {
        self.inactivity_seconds = 0;
        self.power_mode = BacklightPowerMode::On;
        self.current_pwm_duty = (self.brightness_pct as u32 * self.max_brightness_pwm as u32 / 100) as u16;
    }
}

pub static BACKLIGHT_MANAGER: Spinlock<BacklightManager> = Spinlock::new(BacklightManager::new());

pub fn backlight_init() {
    let mut bl = BACKLIGHT_MANAGER.lock();
    bl.set_brightness(85);
}
