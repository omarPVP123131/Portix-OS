// kernel/src/drivers/gpu/mod.rs — Pila Gráfica Nativa en Rust de Portix OS
// DRM/KMS, Asignador de Memoria VRAM TTM/GEM, Vallas de Sincronización y Anillos DMA.

pub mod drm;
pub mod gem_ttm;
pub mod fence;
pub mod ring;
pub mod vaapi;
pub mod gamemode_multimon;

use alloc::string::String;

pub fn init_gpu_subsystem() {
    drm::init_drm();
    gem_ttm::init_ttm();
    fence::init_fences();
    vaapi::init_vaapi();
    gamemode_multimon::init_gaming_gpu();
}

pub fn get_gpu_subsystem_summary() -> String {
    let drm_info = drm::get_drm_summary();
    let ttm_info = gem_ttm::get_ttm_summary();
    let ring_info = ring::get_ring_summary();
    let vaapi_info = vaapi::get_vaapi_summary();
    let game_info = gamemode_multimon::get_gaming_summary();
    alloc::format!("{}\n{}\n{}\n{}\n{}", drm_info, ttm_info, ring_info, vaapi_info, game_info)
}
