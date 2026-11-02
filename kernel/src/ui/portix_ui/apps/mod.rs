//! Suite de Aplicaciones Nativas Diarias de Portix OS.
//!
//! Incluye Portix-Files, Portix-Term, Portix-Monitor y Portix-Settings.

pub mod files;
pub mod terminal;
pub mod monitor;
pub mod settings;

pub fn init_daily_apps() {
    files::init_portix_files();
    terminal::init_portix_term();
    monitor::init_portix_monitor();
    settings::init_portix_settings();
}
