// ui/tabs/terminal.rs — Pestaña TERMINAL: historial, input, barra de scroll

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::console::terminal::{Terminal, LineColor, TERM_ROWS, SCROLL_STEP};
use crate::ui::SCROLLBAR_W;

/// Devuelve (hist_top, hist_h, input_y, max_lines) para la geometría del
/// área de historial y la caja de input. Usado tanto aquí como en main para
/// el manejo de eventos de ratón/teclado.
pub fn terminal_hist_geometry(lay: &Layout) -> (usize, usize, usize, usize) {
    let input_h  = 24usize;
    let input_y  = lay.bottom_y.saturating_sub(input_h + 4);
    let hist_top = lay.content_y + 22;
    let hist_h   = input_y.saturating_sub(hist_top + 2);
    let max_lines = hist_h / lay.line_h;
    (hist_top, hist_h, input_y, max_lines)
}

pub fn draw_terminal_tab(
    c: &mut Console,
    lay: &Layout,
    term: &Terminal,
    sb_dragging: bool,
) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::TERM_BG);

    // ── Barra de título de la terminal ────────────────────────────────────
    c.fill_rect(0, cy, fw, 18, Color::new(2, 8, 18));
    c.hline(0, cy + 17, fw, Color::new(16, 32, 60));
    c.fill_rect(pad,      cy + 4, 8, 8, Color::GREEN);
    c.fill_rect(pad + 14, cy + 4, 8, 8, Color::PORTIX_AMBER);
    c.fill_rect(pad + 28, cy + 4, 8, 8, Color::RED);
    c.write_at("PORTIX TERMINAL v0.7", pad + 46, cy + 5, Color::PORTIX_AMBER);
    c.write_at("Rueda/RePag=scroll  ESC=limpiar",
               fw.saturating_sub(280), cy + 5, Color::new(32, 48, 68));

    let (hist_top, hist_h, input_y, max_lines) = terminal_hist_geometry(lay);

    // Borde izquierdo decorativo
    for y in (hist_top..input_y).step_by(2) {
        c.fill_rect(0, y, 3, 1, Color::new(0, 5, 10));
    }

    // ── Scrollbar ─────────────────────────────────────────────────────────
    let sb_x = fw.saturating_sub(SCROLLBAR_W);

    if term.line_count > max_lines {
        c.fill_rect(sb_x, hist_top, SCROLLBAR_W, hist_h, Color::new(4, 10, 20));

        let max_scroll = term.max_scroll(max_lines);
        let available  = term.line_count.saturating_sub(
            if term.line_count > TERM_ROWS { term.line_count - TERM_ROWS } else { 0 }
        );
        let thumb_h  = if available == 0 { hist_h }
                       else { (hist_h * max_lines / available).max(10).min(hist_h) };
        let travel   = hist_h.saturating_sub(thumb_h);
        let thumb_top = if max_scroll == 0 {
            hist_top + travel
        } else {
            let oc = term.scroll_offset.min(max_scroll);
            hist_top + travel - (travel * oc / max_scroll)
        };

        let thumb_col = if sb_dragging            { Color::PORTIX_GOLD  }
                        else if term.at_bottom()  { Color::TEAL         }
                        else                      { Color::PORTIX_AMBER };

        c.fill_rect(sb_x,                     thumb_top, 2,               thumb_h, Color::new(8, 20, 40));
        c.fill_rect(sb_x + 2,                 thumb_top, SCROLLBAR_W - 4, thumb_h, thumb_col);
        c.fill_rect(sb_x + SCROLLBAR_W - 2,   thumb_top, 2,               thumb_h, Color::new(8, 20, 40));

        if !term.at_bottom() {
            let bx = sb_x.saturating_sub(82);
            c.fill_rounded(bx, hist_top + 4, 78, 14, 3, Color::new(20, 40, 0));
            c.write_at("arrib SCROLL", bx + 4, hist_top + 6, Color::PORTIX_GOLD);
        }
    } else {
        c.fill_rect(sb_x, hist_top, SCROLLBAR_W, hist_h, Color::new(2, 6, 12));
    }

    // ── Historial visible ─────────────────────────────────────────────────
    let (start, count)  = term.visible_range(max_lines);
    let text_area_w     = sb_x.saturating_sub(pad + 4);

    for i in 0..count {
        let line = term.line_at(start + i);
        if line.len == 0 { continue; }
        let ly = hist_top + i * lay.line_h;
        if ly + lay.line_h > input_y { break; }

        let col = match line.color {
            LineColor::Success => Color::NEON_GREEN,
            LineColor::Warning => Color::PORTIX_AMBER,
            LineColor::Error   => Color::RED,
            LineColor::Info    => Color::CYAN,
            LineColor::Prompt  => Color::PORTIX_GOLD,
            LineColor::Header  => Color::WHITE,
            LineColor::Normal  => Color::LIGHT_GRAY,
        };

        let s = core::str::from_utf8(&line.buf[..line.len.min(text_area_w / 9 + 1)])
            .unwrap_or("");

        if line.color == LineColor::Prompt {
            c.fill_rect(0, ly - 1, fw, lay.line_h + 1, Color::new(5, 12, 22));
        }
        c.write_at(s, pad + 4, ly, col);
    }

    // ── Línea de input ────────────────────────────────────────────────────
    c.fill_rect(0, input_y - 2, fw, 2,  Color::new(12, 28, 52));
    c.fill_rect(0, input_y,     fw, 24, Color::new(2, 10, 22));

    let prompt = "PORTIX> ";
    c.write_at(prompt, pad, input_y + 8, Color::PORTIX_GOLD);

    let ix         = pad + prompt.len() * 9;
    let input_str  = core::str::from_utf8(&term.input[..term.input_len]).unwrap_or("");
    c.write_at(input_str, ix, input_y + 8, Color::WHITE);

    let cur_x = ix + term.input_len * 9;
    if term.cursor_vis && cur_x + 7 < sb_x {
        c.fill_rect(cur_x, input_y + 6, 7, 13, Color::PORTIX_GOLD);
    }

    // Evitar warnings de importaciones no usadas en algunas configuraciones
    let _ = SCROLL_STEP;
}
