// ui/tabs/mod.rs — PORTIX Kernel v0.7.4

pub mod system;
pub mod terminal;
pub mod devices;
pub mod ide;
pub mod explorer;

pub use system::draw_system_tab;
pub use terminal::draw_terminal_tab;
pub use devices::draw_devices_tab;
pub use ide::draw_ide_tab;
pub use explorer::draw_explorer_tab;