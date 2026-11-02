// kernel/src/ui/portix_ui/mod.rs — Toolkit GUI Reactivo Nativo y Shell de Escritorio Portix-UI
// Fases 651 - 700 (Portix-UI) & 701 - 750 (Portix Desktop Environment)

pub mod theme;
pub mod gestures;
pub mod desktop_bar;
pub mod runner;
pub mod lockscreen;
pub mod apps;

use alloc::string::String;

pub fn init_portix_ui() {
    desktop_bar::init_desktop_bar();
    runner::init_runner();
    lockscreen::init_lockscreen();
    apps::init_daily_apps();
}

pub fn get_ui_summary() -> String {
    let theme_str = theme::get_theme_summary();
    let gestures_str = gestures::get_gestures_summary();
    let bar_str = desktop_bar::get_bar_summary();
    let runner_str = runner::get_runner_summary();
    let dm_str = lockscreen::get_dm_summary();

    alloc::format!(
        "{}\n{}\n{}\n{}\n{}",
        theme_str, gestures_str, bar_str, runner_str, dm_str
    )
}
