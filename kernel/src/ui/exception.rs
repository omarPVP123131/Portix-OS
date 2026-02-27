// ui/exception.rs — Pantalla de error/excepción de kernel

use crate::graphics::driver::framebuffer::{Color, Console};

/// Muestra una pantalla de pánico azul con título e info de la excepción.
/// Llama a `c.present()` antes de detener el sistema.
pub fn draw_exception(c: &mut Console, title: &str, info: &str) {
    let w = c.width();
    let h = c.height();

    c.fill_rect(0, 0, w, h, Color::new(0, 0, 60));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h.saturating_sub(4), w, 4, Color::RED);

    let pw = 520; let ph = 130;
    let px = (w - pw) / 2; let py = (h - ph) / 2;
    c.fill_rounded(px, py, pw, ph, 6, Color::new(20, 0, 0));
    c.draw_rect(px, py, pw, ph, 1, Color::RED);

    c.write_at("!!! EXCEPCION DE KERNEL !!!", px + pw / 2 - 120, py + 12, Color::RED);
    c.hline(px + 10, py + 30, pw - 20, Color::new(80, 20, 20));
    c.write_at(title, px + 14, py + 42, Color::WHITE);
    c.write_at(info,  px + 14, py + 60, Color::LIGHT_GRAY);
    c.write_at("Sistema detenido. Por favor reinicia.", px + 14, py + 88, Color::GRAY);

    c.present();
}
