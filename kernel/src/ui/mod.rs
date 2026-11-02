// kernel/src/ui/mod.rs — PORTIX Modern Desktop Environment

pub mod exception;
pub mod input;
pub mod protocol;
pub mod window;
pub mod font_ttf;
pub mod widgets;
pub mod wayland_server;
pub mod wayland_effects;
pub mod wayland_seat;
pub mod tiling_wm;
pub mod web_engine;
pub mod web_accel;
pub mod portix_ui;
pub mod commercial_oobe;

pub use window::{WindowManager, Window, WindowKind};
pub use protocol::{CompositorEvent, EventQueue, PtyStream, WindowSurface, SurfaceId};
