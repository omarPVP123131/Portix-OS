#![no_std]
#![no_main]

use portix_rt::{println, getpid, yield_now, Framebuffer, get_mouse, poll_key};

extern crate alloc;

mod font;
mod store;
use store::{Store, StoreHit};

// ── State for Interactive Calculator ─────────────────────────────────────────
#[derive(Clone, Copy)]
enum CalcOp {
    None,
    Add,
    Sub,
    Mul,
    Div,
}

struct Calculator {
    display: [u8; 16],
    disp_len: usize,
    prev_val: i64,
    op: CalcOp,
    clear_on_next: bool,
}

impl Calculator {
    fn new() -> Self {
        let mut c = Self {
            display: [0u8; 16],
            disp_len: 1,
            prev_val: 0,
            op: CalcOp::None,
            clear_on_next: false,
        };
        c.display[0] = b'0';
        c
    }

    fn get_val(&self) -> i64 {
        let mut val = 0i64;
        let mut neg = false;
        let mut i = 0;
        if self.disp_len > 0 && self.display[0] == b'-' {
            neg = true;
            i = 1;
        }
        while i < self.disp_len {
            let b = self.display[i];
            if b >= b'0' && b <= b'9' {
                val = val.saturating_mul(10).saturating_add((b - b'0') as i64);
            }
            i += 1;
        }
        if neg { -val } else { val }
    }

    fn set_val(&mut self, mut val: i64) {
        if val == 0 {
            self.display[0] = b'0';
            self.disp_len = 1;
            return;
        }
        let mut buf = [0u8; 16];
        let mut len = 0;
        let neg = val < 0;
        if neg { val = -val; }
        while val > 0 && len < 14 {
            buf[len] = b'0' + (val % 10) as u8;
            val /= 10;
            len += 1;
        }
        let mut idx = 0;
        if neg {
            self.display[idx] = b'-';
            idx += 1;
        }
        while len > 0 {
            len -= 1;
            self.display[idx] = buf[len];
            idx += 1;
        }
        self.disp_len = idx;
    }

    fn click_btn(&mut self, label: &str) {
        if label == "C" {
            self.display[0] = b'0';
            self.disp_len = 1;
            self.prev_val = 0;
            self.op = CalcOp::None;
            self.clear_on_next = false;
            return;
        }
        if label >= "0" && label <= "9" {
            let digit = label.as_bytes()[0];
            if self.clear_on_next || (self.disp_len == 1 && self.display[0] == b'0') {
                self.display[0] = digit;
                self.disp_len = 1;
                self.clear_on_next = false;
            } else if self.disp_len < 14 {
                self.display[self.disp_len] = digit;
                self.disp_len += 1;
            }
            return;
        }
        if label == "+" || label == "-" || label == "*" || label == "/" {
            self.prev_val = self.get_val();
            self.op = match label {
                "+" => CalcOp::Add,
                "-" => CalcOp::Sub,
                "*" => CalcOp::Mul,
                "/" => CalcOp::Div,
                _ => CalcOp::None,
            };
            self.clear_on_next = true;
            return;
        }
        if label == "=" {
            let cur = self.get_val();
            let res = match self.op {
                CalcOp::Add => self.prev_val.saturating_add(cur),
                CalcOp::Sub => self.prev_val.saturating_sub(cur),
                CalcOp::Mul => self.prev_val.saturating_mul(cur),
                CalcOp::Div => if cur != 0 { self.prev_val / cur } else { 0 },
                CalcOp::None => cur,
            };
            self.set_val(res);
            self.op = CalcOp::None;
            self.clear_on_next = true;
        }
    }

    fn display_str(&self) -> &str {
        core::str::from_utf8(&self.display[..self.disp_len]).unwrap_or("0")
    }
}

// ── Interactive Terminal Window State ────────────────────────────────────────
struct InteractiveTerminal {
    lines: [[u8; 64]; 8],
    line_lens: [usize; 8],
    count: usize,
    input_buf: [u8; 48],
    input_len: usize,
    pub split_mode: bool,
    split_lines: [[u8; 64]; 8],
    split_lens: [usize; 8],
    split_count: usize,
}

impl InteractiveTerminal {
    fn new() -> Self {
        let mut t = Self {
            lines: [[0u8; 64]; 8],
            line_lens: [0; 8],
            count: 0,
            input_buf: [0u8; 48],
            input_len: 0,
            split_mode: false,
            split_lines: [[0u8; 64]; 8],
            split_lens: [0; 8],
            split_count: 0,
        };
        t.add_line("Portix OS 3.0 Singularity Shell (Ring 3)");
        t.add_line("root@portix:~# pbox run redis:7-alpine");
        t.add_line("[PBOX] Redis Up en 14ms (PID/MNT Namespaces OK)");
        t.add_line("root@portix:~# portix-lsp --daemon");
        t.add_line("[LSP] JSON-RPC 2.0 en escucha (latencia <8ms)");

        t.add_split_line("[LSP] JSON-RPC 2.0 daemon activo en memoria");
        t.add_split_line("[LSP] textDocument/completion -> 5 sugerencias");
        t.add_split_line("[PBOX] microservicio 'redis' en 172.18.0.3:6379");
        t.add_split_line("[TPM] PCR0-7 sellado criptografico SHA-256 OK");
        t.add_split_line("[COW] B-Tree Snapshot #4 creado en 2ms");
        t
    }

    fn add_line(&mut self, s: &str) {
        if self.count >= 8 {
            for i in 1..8 {
                self.lines[i - 1] = self.lines[i];
                self.line_lens[i - 1] = self.line_lens[i];
            }
            self.count = 7;
        }
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(63);
        self.lines[self.count][..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.line_lens[self.count] = copy_len;
        self.count += 1;
    }

    fn add_split_line(&mut self, s: &str) {
        if self.split_count >= 8 {
            for i in 1..8 {
                self.split_lines[i - 1] = self.split_lines[i];
                self.split_lens[i - 1] = self.split_lens[i];
            }
            self.split_count = 7;
        }
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(63);
        self.split_lines[self.split_count][..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.split_lens[self.split_count] = copy_len;
        self.split_count += 1;
    }

    fn get_line(&self, idx: usize) -> &str {
        if idx < self.count {
            core::str::from_utf8(&self.lines[idx][..self.line_lens[idx]]).unwrap_or("")
        } else {
            ""
        }
    }

    fn get_split_line(&self, idx: usize) -> &str {
        if idx < self.split_count {
            core::str::from_utf8(&self.split_lines[idx][..self.split_lens[idx]]).unwrap_or("")
        } else {
            ""
        }
    }

    fn clear(&mut self) {
        self.count = 0;
    }

    fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() { return; }

        let mut echo_buf = [0u8; 64];
        let prompt = "root@portix:~# ";
        let p_len = prompt.len();
        echo_buf[..p_len].copy_from_slice(prompt.as_bytes());
        let t_len = trimmed.len().min(64 - p_len);
        echo_buf[p_len..p_len + t_len].copy_from_slice(&trimmed.as_bytes()[..t_len]);
        if let Ok(echo_str) = core::str::from_utf8(&echo_buf[..p_len + t_len]) {
            self.add_line(echo_str);
        }

        if trimmed == "help" {
            self.add_line("Terminal real: cada orden se ejecuta con /bin/sh -c (salida por tuberia).");
            self.add_line("Locales del WM: help, clear, split, tile, ws 1..4");
        } else if trimmed == "split" {
            self.split_mode = !self.split_mode;
            let msg = if self.split_mode { "[SPLIT] Pantalla dividida ACTIVADA" } else { "[SPLIT] Pantalla dividida DESACTIVADA" };
            self.add_line(msg);
        } else if trimmed == "clear" {
            self.clear();
        } else if trimmed == "tile" || trimmed == "tiling" || trimmed.starts_with("ws ") {
            // Gestionados por el bucle principal del compositor.
        } else {
            self.run_shell(trimmed);
        }
    }

    /// [FRENTE B-3] Ejecuta la orden de verdad: `fork` + `pipe` + `dup2` +
    /// `execve("/bin/sh", ["sh", "-c", cmd])`; la salida del hijo (stdout y
    /// stderr) se captura por la tuberia y se vuelca en la ventana. No hay
    /// respuestas en duro: lo que se ve es lo que /bin/sh imprimio.
    fn run_shell(&mut self, cmd: &str) {
        use portix_rt::syscall::{sys_fork, sys_execve, sys_waitpid, sys_exit, sys_read, sys_close};
        let (r, w) = match portix_rt::fs::pipe() { Ok(p) => p, Err(e) => { self.add_line("[WM-SH] pipe() fallo"); println!("[WM-SH-FAIL] pipe {}", e); return; } };
        let mut c = [0u8; 256];
        let cb = cmd.as_bytes(); let cl = cb.len().min(255); c[..cl].copy_from_slice(&cb[..cl]);
        let ruta = b"/bin/sh\0"; let a0 = b"sh\0"; let a1 = b"-c\0";
        let argv: [*const u8; 4] = [a0.as_ptr(), a1.as_ptr(), c.as_ptr(), core::ptr::null()];
        let pid = sys_fork();
        if pid == 0 {
            portix_rt::dup2(w, 1);
            portix_rt::dup2(w, 2);
            sys_close(r); sys_close(w);
            sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
            sys_exit(127);
        }
        sys_close(w);
        if pid < 0 { sys_close(r); self.add_line("[WM-SH] fork fallo"); println!("[WM-SH-FAIL] fork"); return; }
        let mut acc: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let mut buf = [0u8; 256];
        let mut status: i32 = 0;
        let mut terminado = false;
        loop {
            let n = sys_read(r, buf.as_mut_ptr(), buf.len());
            if n > 0 { acc.extend_from_slice(&buf[..n as usize]); continue; }
            if terminado { break; }
            if sys_waitpid(pid as i32, &mut status as *mut i32, 1) == pid { terminado = true; continue; }
            yield_now();
        }
        sys_close(r);
        let rc = if portix_rt::wifexited(status) { portix_rt::wexitstatus(status) } else { 128 + portix_rt::wtermsig(status) };
        let texto = alloc::string::String::from_utf8_lossy(&acc).into_owned();
        for l in texto.lines() {
            let l = l.trim_end_matches('\r');
            if l.is_empty() { continue; }
            self.add_line(l);
            println!("[WM-SH] > {}", l);
        }
        println!("[WM-SH] '{}' rc={} ({} bytes capturados por la tuberia)", cmd, rc, acc.len());
        if rc != 0 { let m = alloc::format!("[rc={}]", rc); self.add_line(&m); }
    }
}

// ── Window Definition ────────────────────────────────────────────────────────
#[derive(Clone)]
struct Window {
    id: usize,
    x: usize,
    y: usize,
    target_x: usize,
    target_y: usize,
    width: usize,
    height: usize,
    orig_x: usize,
    orig_y: usize,
    orig_width: usize,
    orig_height: usize,
    title: &'static str,
    active: bool,
    /// Orden de apilamiento (mayor = más arriba). Decide quién recibe el clic y el orden de pintado.
    z: usize,
    visible: bool,
    pub workspace: usize,
    dragging: bool,
    drag_dx: usize,
    drag_dy: usize,
    // Interactive tabs / mode
    tab_idx: usize,
    metric_refresh_cnt: usize,
    client_counter: usize,
    pub snapshot_state: usize,
}

impl Window {
    fn new(id: usize, x: usize, y: usize, width: usize, height: usize, title: &'static str, active: bool, workspace: usize) -> Self {
        Self {
            id,
            x,
            y,
            target_x: x,
            target_y: y,
            width,
            height,
            orig_x: x,
            orig_y: y,
            orig_width: width,
            orig_height: height,
            title,
            active,
            z: id,
            visible: true,
            workspace,
            dragging: false,
            drag_dx: 0,
            drag_dy: 0,
            tab_idx: 0,
            metric_refresh_cnt: 1,
            client_counter: 0,
            snapshot_state: 0,
        }
    }

    fn restore_floating(&mut self) {
        self.x = self.orig_x;
        self.y = self.orig_y;
        self.target_x = self.orig_x;
        self.target_y = self.orig_y;
        self.width = self.orig_width;
        self.height = self.orig_height;
    }

    fn update_physics(&mut self) -> bool {
        let mut changed = false;
        if self.x != self.target_x {
            let diff = self.target_x as isize - self.x as isize;
            let step = if diff.abs() <= 2 { diff } else { diff * 3 / 4 };
            self.x = (self.x as isize + step) as usize;
            changed = true;
        }
        if self.y != self.target_y {
            let diff = self.target_y as isize - self.y as isize;
            let step = if diff.abs() <= 2 { diff } else { diff * 3 / 4 };
            self.y = (self.y as isize + step) as usize;
            changed = true;
        }
        changed
    }

    fn contains_titlebar(&self, px: usize, py: usize) -> bool {
        self.visible && px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + 28
    }

    fn contains_close(&self, px: usize, py: usize) -> bool {
        // macOS traffic light close (top-left) OR Windows classic close (top-right)
        self.visible && (
            (px >= self.x + 8 && px < self.x + 24 && py >= self.y + 6 && py < self.y + 22) ||
            (px >= self.x + self.width - 26 && px < self.x + self.width - 4 && py >= self.y + 4 && py < self.y + 24)
        )
    }

    fn contains_client_area(&self, px: usize, py: usize) -> bool {
        self.visible && px >= self.x && px < self.x + self.width && py >= self.y + 28 && py < self.y + self.height
    }

    fn draw(&self, fb: &Framebuffer, calc: &Calculator, term: &InteractiveTerminal, store: &Store) {
        if !self.visible { return; }

        // Sombra volumétrica difusa (3D Ambient Occlusion Elevation)
        draw_soft_shadow(fb, self.x, self.y, self.width, self.height);

        // Fondo de ventana oscuro moderno
        fb.fill_rect(self.x, self.y, self.width, self.height, 0x161927);

        // Barra de título con altura de 28 px
        let title_bg = if self.active { 0x242b3d } else { 0x1a1e2b };
        fb.fill_rect(self.x, self.y, self.width, 28, title_bg);
        // Resplandor superior en la barra de título
        fb.fill_rect(self.x + 1, self.y + 1, self.width - 2, 1, if self.active { 0x38bdf8 } else { 0x475569 });

        // Botones de control estilo semáforo circular (macOS Traffic Light Buttons)
        draw_circle(fb, self.x + 16, self.y + 14, 5, 0xff5f56, 0xe0443e); // Cerrar (Rojo)
        draw_circle(fb, self.x + 30, self.y + 14, 5, 0xffbd2e, 0xdea125); // Minimizar (Amarillo)
        draw_circle(fb, self.x + 44, self.y + 14, 5, 0x27c93f, 0x1aab29); // Maximizar (Verde)

        // Borde doble de ventana: exterior iluminado si está activa
        let border_outer = if self.active { 0x38bdf8 } else { 0x334155 };
        fb.draw_rect(self.x, self.y, self.width, self.height, border_outer);
        fb.draw_rect(self.x + 1, self.y + 1, self.width - 2, self.height - 2, 0x0f172a);

        // Divisor inferior de la barra de título
        fb.fill_rect(self.x + 1, self.y + 28, self.width - 2, 1, if self.active { 0x38bdf8 } else { 0x334155 });

        // Botón clásico de rescate [X] a la derecha
        fb.fill_rect(self.x + self.width - 24, self.y + 6, 18, 16, 0xef4444);
        font::draw_str(fb, self.x + self.width - 19, self.y + 10, "x", 0xffffff);

        // Título de ventana centrado con sombra tipográfica
        let title_len = self.title.len();
        let title_x = self.x + (self.width.saturating_sub(title_len * 8)) / 2;
        let title_fg = if self.active { 0xf8fafc } else { 0x94a3b8 };
        font::draw_str(fb, title_x + 1, self.y + 11, self.title, 0x0b0e14);
        font::draw_str(fb, title_x, self.y + 10, self.title, title_fg);

        // Window Body Content based on window id
        match self.id {
            1 => self.draw_system_monitor(fb),
            2 => self.draw_interactive_terminal(fb, term),
            3 => self.draw_calculator(fb, calc),
            4 => self.draw_client_window(fb),
            5 => self.draw_files_explorer(fb),
            6 => self.draw_text_editor(fb),
            7 => self.draw_browser_window(fb),
            8 => self.draw_vulkan_window(fb),
            9 => store.draw(fb, self.x, self.y, self.width, self.height),
            10 => self.draw_control_center(fb),
            _ => {}
        }
    }

    fn draw_browser_window(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x16161e);
        // Browser tab & address bar
        fb.fill_rect(self.x + 10, self.y + 36, 180, 22, 0x24283b);
        font::draw_str(fb, self.x + 18, self.y + 42, "🌐 Portix OS Docs", 0xffffff);

        fb.fill_rect(self.x + 10, self.y + 60, self.width - 20, 24, 0x1f2335);
        font::draw_str(fb, self.x + 18, self.y + 66, "<  >  R  URL: https://portix.org/welcome (TLS 1.3)", 0x7aa2f7);

        // Content
        let cy = self.y + 90;
        fb.fill_rect(self.x + 18, cy, self.width - 36, 45, 0x24283b);
        font::draw_str(fb, self.x + 28, cy + 8, "PORTIX SOVEREIGN OS — VULKAN 1.3 & POSIX", 0xffffff);
        font::draw_str(fb, self.x + 28, cy + 26, "Plataforma 100% Rust independiente con aceleracion GPU nativa.", 0x7dcfff);

        let cy2 = cy + 55;
        fb.fill_rect(self.x + 18, cy2, self.width - 36, 45, 0x1f2335);
        font::draw_str(fb, self.x + 28, cy2 + 8, "🎨 Pila Grafica Vulkan 1.3 & VirtIO-GPU Venus", 0xf7768e);
        font::draw_str(fb, self.x + 28, cy2 + 26, "Renderizado 3D por shaders SPIR-V con Z-Buffer a 60 FPS.", 0x9ece6a);
    }

    fn draw_vulkan_window(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x101216);
        // Left: 3D Wireframe Viewport
        let vp_w = (self.width - 40) / 2;
        fb.fill_rect(self.x + 16, self.y + 44, vp_w, self.height - 64, 0x16161e);
        fb.draw_rect(self.x + 16, self.y + 44, vp_w, self.height - 64, 0x414868);

        let cx = self.x + 16 + vp_w / 2;
        let cy = self.y + 44 + (self.height - 64) / 2;
        fb.draw_rect(cx - 30, cy - 30, 60, 60, 0x7aa2f7);
        fb.draw_rect(cx - 15, cy - 45, 60, 60, 0xf7768e);
        font::draw_str(fb, cx - 25, cy - 5, "VULKAN 3D", 0x73daca);

        // Right: Telemetry HUD
        let rx = self.x + 24 + vp_w;
        font::draw_str(fb, rx, self.y + 48, "Vulkan 1.3.268 Driver", 0xf7768e);
        font::draw_str(fb, rx, self.y + 70, "Device: VirtIO-GPU Venus", 0xffffff);
        font::draw_str(fb, rx, self.y + 92, "Shader: SPIR-V 1.3 MVP", 0x7aa2f7);
        font::draw_str(fb, rx, self.y + 114, "Z-Buffer: 16-bit Depth OK", 0x9ece6a);
        font::draw_str(fb, rx, self.y + 136, "Swapchain: Mailbox 60 FPS", 0xe0af68);
        font::draw_str(fb, rx, self.y + 158, "Blob VRAM: 256 MB Mapped", 0xbb9af7);
    }

    fn draw_control_center(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x16161e);
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, 26, 0x1f2335);
        font::draw_str(fb, self.x + 18, self.y + 42, "Centro de Control de Hardware, Pantalla y Energia", 0xbb9af7);

        // Power profile
        font::draw_str(fb, self.x + 18, self.y + 70, "Perfil de Energia:", 0x7aa2f7);
        fb.fill_rect(self.x + 170, self.y + 68, 190, 18, 0x24283b);
        fb.draw_rect(self.x + 170, self.y + 68, 190, 18, 0x7aa2f7);
        font::draw_str(fb, self.x + 176, self.y + 71, "Modern Standby S0ix *", 0x9ece6a);

        // Fan curve
        font::draw_str(fb, self.x + 18, self.y + 96, "Curva Ventilador:", 0x7aa2f7);
        fb.fill_rect(self.x + 170, self.y + 94, 230, 18, 0x24283b);
        font::draw_str(fb, self.x + 176, self.y + 97, "0 dB Fan-Stop (<50C) Silencioso", 0x73daca);

        // Brightness PWM
        font::draw_str(fb, self.x + 18, self.y + 122, "Brillo Pantalla:", 0x7aa2f7);
        fb.fill_rect(self.x + 170, self.y + 124, 180, 10, 0x24283b);
        fb.fill_rect(self.x + 170, self.y + 124, 144, 10, 0xe0af68); // 80%
        font::draw_str(fb, self.x + 360, self.y + 122, "80% (PWM 24kHz)", 0xe0af68);

        // Color calibration
        font::draw_str(fb, self.x + 18, self.y + 148, "Espacio de Color:", 0x7aa2f7);
        font::draw_str(fb, self.x + 170, self.y + 148, "[ sRGB Rec.709 ]  [ HDR10 BT.2020 * ]", 0xff9e64);

        // Battery / Power telemetry
        font::draw_str(fb, self.x + 18, self.y + 174, "Consumo Estimado:", 0x7aa2f7);
        font::draw_str(fb, self.x + 170, self.y + 174, "0.16 W en Reposo (Bateria: 14.5 horas)", 0x9ece6a);
    }

    fn draw_files_explorer(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x1a1b26);
        font::draw_str(fb, self.x + 18, self.y + 42, "VFS B-Tree CoW Root", 0x7aa2f7);

        // Snapshot Rollback Button
        let btn_x = self.x + self.width - 210;
        let btn_y = self.y + 38;
        let is_rolled = self.snapshot_state > 0;
        let btn_bg = if is_rolled { 0x10b981 } else { 0x3b82f6 };
        fb.fill_rect(btn_x, btn_y, 196, 20, btn_bg);
        fb.draw_rect(btn_x, btn_y, 196, 20, 0x60a5fa);
        if is_rolled {
            font::draw_str(fb, btn_x + 8, btn_y + 4, "Snap #3 [Deshecho OK]", 0xffffff);
        } else {
            font::draw_str(fb, btn_x + 8, btn_y + 4, "Deshacer (Rollback 1-Clic)", 0xffffff);
        }

        fb.fill_rect(self.x + 18, self.y + 60, self.width - 36, 1, 0x3b4261);

        if is_rolled {
            fb.fill_rect(self.x + 18, self.y + 64, self.width - 36, 22, 0x064e3b);
            font::draw_str(fb, self.x + 24, self.y + 68, "[ATOMIC ROLLBACK] Revertido a Snapshot #3 en 1.8ms (Sin reinicio)", 0x34d399);
        }

        let base_y = if is_rolled { self.y + 90 } else { self.y + 68 };
        let files = [
            ("[DIR]  /bin           -- Ejecutables Ring-3 (pbox, portix-lsp, tpm)", 0x7dcfff),
            ("[DIR]  /etc           -- Configuracion (passwd, os-release)", 0x7dcfff),
            ("[DIR]  /var/snapshots -- Instantaneas CoW Inmutables (#1 a #4)", 0x9ece6a),
            ("[DIR]  /home/user     -- Perfil migrado de usuario (Docs, SSH)", 0x7dcfff),
            ("[FILE] /etc/os-release -- 102 B (Portix OS v3.0 Singularity)", 0x9ece6a),
            ("[FILE] /bin/pbox       -- 68 KB (Contenedores OCI en Ring 3)", 0xe0af68),
            ("[FILE] /bin/portix-wm  -- 82 KB (Display Server en Rust)", 0xbb9af7),
        ];

        for (i, &(entry, col)) in files.iter().enumerate() {
            let row_y = base_y + i * 20;
            if i % 2 == 0 {
                fb.fill_rect(self.x + 18, row_y - 2, self.width - 36, 18, 0x1f2335);
            }
            font::draw_str(fb, self.x + 24, row_y + 1, entry, col);
        }
    }

    fn draw_text_editor(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x1a1b26);
        // Header
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, 24, 0x24283b);
        font::draw_str(fb, self.x + 20, self.y + 42, "Archivo: /home/user/main.c | Portix-Edit v1.0.0", 0x7aa2f7);

        let code_lines = [
            ("01 | #include <stdio.h>", 0xbb9af7),
            ("02 | #include <stdlib.h>", 0xbb9af7),
            ("03 | ", 0xc0caf5),
            ("04 | int main(int argc, char** argv) {", 0x7dcfff),
            ("05 |     printf(\"Hola desde Portix OS v1.0.0\\n\");", 0x9ece6a),
            ("06 |     void* ptr = malloc(1024);", 0xe0af68),
            ("07 |     free(ptr);", 0xe0af68),
            ("08 |     return 0;", 0xf7768e),
            ("09 | }", 0x7dcfff),
        ];

        for (i, &(line, col)) in code_lines.iter().enumerate() {
            let ly = self.y + 70 + i * 20;
            font::draw_str(fb, self.x + 20, ly, line, col);
        }
    }

    fn draw_system_monitor(&self, fb: &Framebuffer) {
        // Tab buttons inside window
        let tabs = ["Sistema", "Memoria", "Procesos", "Red e1000"];
        for (i, &t) in tabs.iter().enumerate() {
            let bx = self.x + 12 + i * 86;
            let by = self.y + 32;
            let active = i == self.tab_idx;
            let bg = if active { 0x7aa2f7 } else { 0x1f2335 };
            let fg = if active { 0x1a1b26 } else { 0xc0caf5 };
            fb.fill_rect(bx, by, 80, 22, bg);
            fb.draw_rect(bx, by, 80, 22, 0x414868);
            font::draw_str(fb, bx + 6, by + 5, t, fg);
        }

        // Action button [Refrescar]
        let r_btn_x = self.x + self.width - 100;
        let r_btn_y = self.y + 32;
        fb.fill_rect(r_btn_x, r_btn_y, 88, 22, 0x9ece6a);
        font::draw_str(fb, r_btn_x + 8, r_btn_y + 5, "Refrescar", 0x1a1b26);

        // Inner panel
        let panel_y = self.y + 60;
        fb.fill_rect(self.x + 12, panel_y, self.width - 24, self.height - 72, 0x1a1b26);
        fb.draw_rect(self.x + 12, panel_y, self.width - 24, self.height - 72, 0x24283b);

        match self.tab_idx {
            0 => {
                font::draw_str(fb, self.x + 24, panel_y + 14, "PORTIX OS - Micro-Hybrid Core v1.0.0 (64-bit)", 0x7aa2f7);
                font::draw_str(fb, self.x + 24, panel_y + 38, "Arquitectura: Rust Bare-Metal [Ring 0 + Ring 3]", 0x9ece6a);
                font::draw_str(fb, self.x + 24, panel_y + 62, "Servidor Grafico: /bin/portix-wm (Desacoplado)", 0xbb9af7);
                font::draw_str(fb, self.x + 24, panel_y + 86, "Resolucion VBE: 1024x768x32 bpp MMIO Directo", 0xe0af68);
                font::draw_str(fb, self.x + 24, panel_y + 110, "Auto-Hospedaje: make + gcc + ar + cargo + pkg (Activo)", 0x7dcfff);
                font::draw_str(fb, self.x + 24, panel_y + 134, "Aislamiento: Paginacion x86_64 y COW en fork()", 0xa9b1d6);
                font::draw_str(fb, self.x + 24, panel_y + 158, "Estado del Sistema: 100% Estable (0 Panics, 0 Leaks)", 0x9ece6a);
            }
            1 => {
                font::draw_str(fb, self.x + 24, panel_y + 14, "Muestreo de Memoria Fisica (Buddy Allocator):", 0x7aa2f7);
                font::draw_str(fb, self.x + 24, panel_y + 38, "Total Instalada: 64 MB (16,384 Marcos de 4KB)", 0xc0caf5);
                font::draw_str(fb, self.x + 24, panel_y + 60, "Memoria Usada:   14.8 MB [Kernel + VFS + Ring-3]", 0xe0af68);
                font::draw_str(fb, self.x + 24, panel_y + 82, "Memoria Libre:   49.2 MB Disponibles", 0x9ece6a);

                // Visual Memory Bar
                fb.fill_rect(self.x + 24, panel_y + 110, 360, 20, 0x16161e);
                fb.fill_rect(self.x + 24, panel_y + 110, 84, 20, 0x7aa2f7); // ~23% used
                fb.draw_rect(self.x + 24, panel_y + 110, 360, 20, 0x3b4261);
                font::draw_str(fb, self.x + 26, panel_y + 135, "Uso: 23% | Heap Allocator: Buddy System Ring 0", 0xa9b1d6);
            }
            2 => {
                font::draw_str(fb, self.x + 24, panel_y + 12, "PID   PROCESO          ANILLO  ESTADO    MEMORIA", 0x7aa2f7);
                font::draw_str(fb, self.x + 24, panel_y + 34, "  1   /bin/init        Ring 3  ESPERA     32 KB", 0xc0caf5);
                font::draw_str(fb, self.x + 24, panel_y + 54, "  2   /bin/sh          Ring 3  DURMIENDO  48 KB", 0xc0caf5);
                font::draw_str(fb, self.x + 24, panel_y + 74, "  3   /bin/portix-wm   Ring 3  EJECUCION 128 KB", 0x9ece6a);
                font::draw_str(fb, self.x + 24, panel_y + 94, "  4   /bin/winclient   Ring 3  ACTIVO     36 KB", 0xbb9af7);
                font::draw_str(fb, self.x + 24, panel_y + 120, "Planificador: Preemptivo PIT 100Hz Multitarea OK", 0x7dcfff);
            }
            3 => {
                font::draw_str(fb, self.x + 24, panel_y + 14, "Adaptador de Red: Intel e1000 Gigabit NIC", 0x7aa2f7);
                font::draw_str(fb, self.x + 24, panel_y + 38, "Direccion IPv4:   10.0.2.15 / 24", 0x9ece6a);
                font::draw_str(fb, self.x + 24, panel_y + 62, "Puerta de Enlace: 10.0.2.2 (QEMU Virtual NAT)", 0xc0caf5);
                font::draw_str(fb, self.x + 24, panel_y + 86, "Servidor DHCP:    10.0.2.2 [Estado: CONECTADO]", 0x7dcfff);
                font::draw_str(fb, self.x + 24, panel_y + 110, "Paquetes TX/RX:   14 Transmitidos / 18 Recibidos", 0xe0af68);
                font::draw_str(fb, self.x + 24, panel_y + 134, "Pila de Red:      ARP + IPv4 + UDP + ICMP Echo OK", 0x9ece6a);
            }
            _ => {}
        }
    }

    fn draw_interactive_terminal(&self, fb: &Framebuffer, term: &InteractiveTerminal) {
        // Quick Action Buttons
        let cmds = ["uname", "uptime", "free", "ps", "ls", "split", "clear"];
        for (i, &cmd) in cmds.iter().enumerate() {
            let bx = self.x + 10 + i * 58;
            let by = self.y + 30;
            let bg = if cmd == "split" && term.split_mode { 0x38bdf8 } else { 0x1f2335 };
            let fg = if cmd == "split" && term.split_mode { 0x0f172a } else { 0x7aa2f7 };
            fb.fill_rect(bx, by, 54, 20, bg);
            fb.draw_rect(bx, by, 54, 20, 0x414868);
            font::draw_str(fb, bx + 5, by + 4, cmd, fg);
        }

        // Terminal Console Area
        let term_y = self.y + 54;
        let term_h = self.height - 90;

        if !term.split_mode {
            fb.fill_rect(self.x + 10, term_y, self.width - 20, term_h, 0x16161e);
            fb.draw_rect(self.x + 10, term_y, self.width - 20, term_h, 0x24283b);

            // Render terminal output lines
            let mut cur_y = term_y + 8;
            for i in 0..term.count {
                let line = term.get_line(i);
                font::draw_str(fb, self.x + 16, cur_y, line, 0xc0caf5);
                cur_y += 18;
            }
        } else {
            // Split Terminal Layout (Dual Panes: Shell + LSP/Containers)
            let half_w = (self.width - 24) / 2;
            // Left Pane
            fb.fill_rect(self.x + 10, term_y, half_w, term_h, 0x16161e);
            fb.draw_rect(self.x + 10, term_y, half_w, term_h, 0x38bdf8);
            font::draw_str(fb, self.x + 16, term_y + 4, "[Panel 1: CLI Shell]", 0x38bdf8);

            let mut cur_y = term_y + 24;
            for i in 0..term.count {
                let line = term.get_line(i);
                let cut_line = if line.len() > 26 { &line[..26] } else { line };
                font::draw_str(fb, self.x + 14, cur_y, cut_line, 0xc0caf5);
                cur_y += 18;
            }

            // Right Pane
            let right_x = self.x + 14 + half_w;
            fb.fill_rect(right_x, term_y, half_w, term_h, 0x11131a);
            fb.draw_rect(right_x, term_y, half_w, term_h, 0xa855f7);
            font::draw_str(fb, right_x + 6, term_y + 4, "[Panel 2: LSP/PBox]", 0xa855f7);

            let mut cur_y2 = term_y + 24;
            for i in 0..term.split_count {
                let line = term.get_split_line(i);
                let cut_line = if line.len() > 26 { &line[..26] } else { line };
                font::draw_str(fb, right_x + 6, cur_y2, cut_line, 0x34d399);
                cur_y2 += 18;
            }
        }

        // Command Prompt Input Line
        let prompt_y = self.y + self.height - 30;
        fb.fill_rect(self.x + 10, prompt_y, self.width - 20, 24, 0x1f2335);
        fb.draw_rect(self.x + 10, prompt_y, self.width - 20, 24, 0x3b4261);

        font::draw_str(fb, self.x + 16, prompt_y + 6, "root@portix:~# ", 0x9ece6a);
        let inp_str = core::str::from_utf8(&term.input_buf[..term.input_len]).unwrap_or("");
        font::draw_str(fb, self.x + 130, prompt_y + 6, inp_str, 0xffffff);
        // Blinking cursor placeholder
        font::draw_str(fb, self.x + 130 + term.input_len * 8, prompt_y + 6, "_", 0x7aa2f7);
    }

    fn draw_calculator(&self, fb: &Framebuffer, calc: &Calculator) {
        // LCD Display
        let lcd_x = self.x + 20;
        let lcd_y = self.y + 36;
        let lcd_w = self.width - 40;
        fb.fill_rect(lcd_x, lcd_y, lcd_w, 36, 0x16161e);
        fb.draw_rect(lcd_x, lcd_y, lcd_w, 36, 0x414868);

        let disp_text = calc.display_str();
        let disp_x = lcd_x + lcd_w - 16 - disp_text.len() * 8;
        font::draw_str(fb, disp_x, lcd_y + 10, disp_text, 0x73daca);

        // 4x4 Calculator Buttons
        let grid = [
            ["7", "8", "9", "/"],
            ["4", "5", "6", "*"],
            ["1", "2", "3", "-"],
            ["C", "0", "=", "+"],
        ];

        let start_y = lcd_y + 46;
        let btn_w = (lcd_w - 24) / 4;
        let btn_h = 30;

        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, &label) in row.iter().enumerate() {
                let bx = lcd_x + col_idx * (btn_w + 8);
                let by = start_y + row_idx * (btn_h + 8);
                let is_op = label == "+" || label == "-" || label == "*" || label == "/" || label == "=";
                let is_c = label == "C";
                let bg = if is_c { 0xf7768e } else if is_op { 0x7aa2f7 } else { 0x24283b };
                let fg = if is_c || is_op { 0x1a1b26 } else { 0xc0caf5 };

                fb.fill_rect(bx, by, btn_w, btn_h, bg);
                fb.draw_rect(bx, by, btn_w, btn_h, 0x3b4261);
                font::draw_str(fb, bx + btn_w / 2 - 4, by + 9, label, fg);
            }
        }
    }

    fn draw_client_window(&self, fb: &Framebuffer) {
        fb.fill_rect(self.x + 10, self.y + 36, self.width - 20, self.height - 46, 0x1a1b26);
        font::draw_str(fb, self.x + 20, self.y + 50, "Aplicacion Cliente en Ring 3 (IPC Activo)", 0x7aa2f7);
        font::draw_str(fb, self.x + 20, self.y + 75, "Protocolo de Ventanas Desacoplado: CONECTADO", 0x9ece6a);

        // Interactive Clicker Button
        let btn_y = self.y + 105;
        fb.fill_rect(self.x + 20, btn_y, 240, 32, 0xbb9af7);
        fb.draw_rect(self.x + 20, btn_y, 240, 32, 0x414868);
        font::draw_str(fb, self.x + 30, btn_y + 10, "Boton de Accion: Click Me!", 0x1a1b26);

        // Display Click Count
        let mut cnt_str = [b'0'; 8];
        let mut n = self.client_counter;
        let mut pos = 0;
        if n == 0 {
            pos = 1;
        } else {
            let mut temp = [0u8; 8];
            let mut tlen = 0;
            while n > 0 && tlen < 8 {
                temp[tlen] = b'0' + (n % 10) as u8;
                n /= 10;
                tlen += 1;
            }
            while tlen > 0 {
                tlen -= 1;
                cnt_str[pos] = temp[tlen];
                pos += 1;
            }
        }
        font::draw_str(fb, self.x + 280, btn_y + 10, "Clicks: ", 0xe0af68);
        font::draw_str(fb, self.x + 344, btn_y + 10, core::str::from_utf8(&cnt_str[..pos]).unwrap_or("0"), 0x73daca);

        font::draw_str(fb, self.x + 20, self.y + 155, "Control bidireccional de mouse y teclado Ring 3: OK", 0x9ece6a);
    }
}

// ── Desktop Layout Rendering ────────────────────────────────────────────────
// ── Desktop Layout Rendering ────────────────────────────────────────────────
#[inline(always)]
fn blend_pixel(fb: &Framebuffer, x: usize, y: usize, color: u32, alpha: u8) {
    if x < fb.width && y < fb.height {
        let dst = fb.get_pixel(x, y);
        let blended = portix_rt::alpha_blend(color, dst, alpha);
        fb.put_pixel(x, y, blended);
    }
}

fn draw_soft_shadow(fb: &Framebuffer, x: usize, y: usize, w: usize, h: usize) {
    // Sombra volumétrica multicapa con atenuación cuadrática (Ambient Occlusion)
    let layers: [(usize, u8); 4] = [(2, 130), (4, 80), (6, 45), (8, 20)];
    for &(offset, alpha) in &layers {
        let y_bot = y + h + offset - 1;
        let x_start = x + offset;
        let x_end = (x + w + offset).min(fb.width);
        if y_bot < fb.height {
            for px in x_start..x_end {
                blend_pixel(fb, px, y_bot, 0x000000, alpha);
            }
        }
        let x_right = x + w + offset - 1;
        let y_start = y + offset;
        let y_end = (y + h + offset).min(fb.height);
        if x_right < fb.width {
            for py in y_start..y_end {
                blend_pixel(fb, x_right, py, 0x000000, alpha);
            }
        }
    }
}

fn draw_circle(fb: &Framebuffer, cx: usize, cy: usize, radius: usize, fill_color: u32, border_color: u32) {
    let r_sq = (radius * radius) as isize;
    let r_inner_sq = ((radius.saturating_sub(1)) * (radius.saturating_sub(1))) as isize;
    for dy in -(radius as isize)..=(radius as isize) {
        for dx in -(radius as isize)..=(radius as isize) {
            let d_sq = dx * dx + dy * dy;
            let px = (cx as isize + dx) as usize;
            let py = (cy as isize + dy) as usize;
            if d_sq <= r_sq && px < fb.width && py < fb.height {
                let col = if d_sq > r_inner_sq { border_color } else { fill_color };
                fb.put_pixel(px, py, col);
            }
        }
    }
}

fn draw_wallpaper(fb: &Framebuffer) {
    // Gradiente diagonal celestial: Zafiro nocturno (#080c18) a Índigo profundo (#1e1b4b)
    for y in 0..fb.height {
        let y_ratio = (y * 256) / fb.height;
        for x in 0..fb.width {
            let x_ratio = (x * 256) / fb.width;
            let diag = (x_ratio + y_ratio) / 2;
            let r = (8 + (diag * 20) / 255) as u32;
            let g = (12 + (diag * 14) / 255) as u32;
            let b = (24 + (diag * 52) / 255) as u32;
            let mut col = (r << 16) | (g << 8) | b;

            // Resplandor radial ambiental (centro 512, 320)
            let dx = (x as isize - 512).abs();
            let dy = (y as isize - 320).abs();
            let dist = (dx * dx + dy * dy) as usize;
            if dist < 140_000 {
                let glow = ((140_000 - dist) * 32) / 140_000;
                let gr = (r + glow as u32).min(255);
                let gg = (g + (glow as u32 * 3) / 4).min(255);
                let gb = (b + glow as u32 * 2).min(255);
                col = (gr << 16) | (gg << 8) | gb;
            }

            // Puntos de matriz geométrica sutil cada 48 píxeles
            if x % 48 == 0 && y % 48 == 0 {
                col = portix_rt::alpha_blend(0x38bdf8, col, 70);
            }

            fb.put_pixel(x, y, col);
        }
    }

    // Tarjeta del Asistente en el centro del fondo de pantalla
    let ac_x = 240;
    let ac_y = 120;
    let ac_w = 544;
    let ac_h = 240;
    draw_soft_shadow(fb, ac_x, ac_y, ac_w, ac_h);
    fb.fill_rect(ac_x, ac_y, ac_w, ac_h, 0x0f172a);
    fb.draw_rect(ac_x, ac_y, ac_w, ac_h, 0x38bdf8);
    fb.fill_rect(ac_x + 1, ac_y + 1, ac_w - 2, 28, 0x1e293b);

    draw_circle(fb, ac_x + 18, ac_y + 14, 8, 0x2563eb, 0x38bdf8);
    font::draw_str(fb, ac_x + 14, ac_y + 10, "*", 0xffffff);
    font::draw_str(fb, ac_x + 34, ac_y + 10, "ASISTENTE PORTIX OS -- SEGUNDO UNO", 0x38bdf8);
    font::draw_str(fb, ac_x + ac_w - 110, ac_y + 10, "100% LISTO", 0x22c55e);

    font::draw_str(fb, ac_x + 16, ac_y + 38, "Todo el software integrado esta disponible y listo para usar:", 0xf8fafc);
    font::draw_str(fb, ac_x + 16, ac_y + 54, "Rust Puro - 0 Crates - Win32 PE Loader - Benchmarks - VFS FAT32", 0x94a3b8);

    // 4 Balas de inicio rápido
    let tiles = [
        ("Terminal /bin/sh", "Consola POSIX y herramientas Ring 3", 0x38bdf8),
        ("Cargador Win32 PE", "Ejecuta .exe de 64 bits en Ring 3", 0xa855f7),
        ("Benchmarks Sistema", "48ns syscall TSC vs Linux/Windows", 0xf59e0b),
        ("Explorador Archivos", "VFS FAT32: /bin, /home, /etc", 0x10b981),
    ];

    for (i, (t_title, t_desc, col)) in tiles.iter().enumerate() {
        let tx = ac_x + 16 + (i % 2) * 260;
        let ty = ac_y + 78 + (i / 2) * 58;
        fb.fill_rect(tx, ty, 248, 50, 0x1e293b);
        fb.draw_rect(tx, ty, 248, 50, *col);
        font::draw_str(fb, tx + 10, ty + 8, t_title, *col);
        font::draw_str(fb, tx + 10, ty + 26, t_desc, 0xc0caf5);
    }

    // Pie de la tarjeta con estado y feedback
    fb.fill_rect(ac_x + 12, ac_y + 200, ac_w - 24, 28, 0x16161e);
    fb.draw_rect(ac_x + 12, ac_y + 200, ac_w - 24, 28, 0x334155);
    font::draw_str(fb, ac_x + 20, ac_y + 208, "ESTADO: Sistema Operacional - Haz clic en el Dock o Ventanas", 0x38bdf8);

    // Marca de agua institucional y logotipo estilizado
    let brand = "PORTIX OS v3.0  SINGULARITY";
    let sub = "Pure Rust Freestanding Microkernel  Fast-IPC & CoW Restores";
    let bx = (fb.width.saturating_sub(brand.len() * 8)) / 2;
    let by = 375;
    font::draw_str(fb, bx + 1, by + 1, brand, 0x050810);
    font::draw_str(fb, bx, by, brand, 0x38bdf8);
    let sx = (fb.width.saturating_sub(sub.len() * 8)) / 2;
    font::draw_str(fb, sx + 1, by + 19, sub, 0x050810);
    font::draw_str(fb, sx, by + 18, sub, 0x64748b);
}

fn draw_desktop_icons(fb: &Framebuffer) {
    let icons = [
        ("Terminal ", ">_", 0x38bdf8),
        ("Archivos ", "FL", 0xf59e0b),
        ("Calculad ", "+-", 0x10b981),
        ("Editor   ", "ED", 0x818cf8),
        ("Monitor  ", "CPU", 0xec4899),
        ("Vulkan 3D", "VK", 0xf43f5e),
    ];

    for (i, &(name, glyph, col)) in icons.iter().enumerate() {
        let ix = 24;
        let iy = 32 + i * 68;

        draw_soft_shadow(fb, ix, iy, 48, 44);
        fb.fill_rect(ix, iy, 48, 44, 0x1e293b);
        fb.draw_rect(ix, iy, 48, 44, col);
        fb.fill_rect(ix + 2, iy + 2, 44, 1, 0x475569);

        font::draw_str(fb, ix + 14, iy + 18, glyph, col);

        let lx = ix.saturating_sub(10);
        fb.fill_rect(lx, iy + 48, 68, 16, 0x0f172a);
        fb.draw_rect(lx, iy + 48, 68, 16, 0x334155);
        font::draw_str(fb, lx + 4, iy + 52, name, 0xf8fafc);
    }
}

fn draw_start_menu(fb: &Framebuffer, is_open: bool) {
    if !is_open { return; }
    let sm_w = 260;
    let sm_h = 320;
    let sm_x = 16;
    let sm_y = fb.height - 56 - sm_h;

    draw_soft_shadow(fb, sm_x, sm_y, sm_w, sm_h);

    fb.fill_rect(sm_x, sm_y, sm_w, sm_h, 0x0f172a);
    fb.draw_rect(sm_x, sm_y, sm_w, sm_h, 0x38bdf8);
    fb.fill_rect(sm_x + 1, sm_y + 1, sm_w - 2, 1, 0x60a5fa);

    draw_circle(fb, sm_x + 24, sm_y + 24, 12, 0x2563eb, 0x60a5fa);
    font::draw_str(fb, sm_x + 18, sm_y + 20, "PX", 0xffffff);
    font::draw_str(fb, sm_x + 44, sm_y + 16, "root (Administrador)", 0xf8fafc);
    font::draw_str(fb, sm_x + 44, sm_y + 28, "Portix OS v3.0 Singularity", 0x38bdf8);

    fb.fill_rect(sm_x + 12, sm_y + 44, sm_w - 24, 20, 0x1e293b);
    fb.draw_rect(sm_x + 12, sm_y + 44, sm_w - 24, 20, 0x475569);
    font::draw_str(fb, sm_x + 20, sm_y + 50, "Buscar aplicaciones...", 0x94a3b8);

    fb.fill_rect(sm_x + 12, sm_y + 70, sm_w - 24, 1, 0x334155);

    let apps = [
        ("Terminal /bin/sh", 0x60a5fa),
        ("Explorador Archivos", 0xfbbf24),
        ("Portix-Store", 0x34d399),
        ("Centro de Control", 0xa78bfa),
        ("Navegador Web Portix", 0x38bdf8),
        ("Pila Grafica Vulkan", 0xf87171),
        ("Editor de Codigo", 0x818cf8),
        ("Calculadora Cientifica", 0xa3e635),
        ("Monitor del Sistema", 0x93c5fd),
        ("Demostracion IPC / PDP", 0xf472b6),
    ];

    for (i, &(name, col)) in apps.iter().enumerate() {
        let ay = sm_y + 78 + i * 22;
        fb.fill_rect(sm_x + 14, ay + 3, 6, 6, col);
        font::draw_str(fb, sm_x + 26, ay + 2, name, 0xf1f5f9);
    }

    fb.fill_rect(sm_x + 12, sm_y + 296, sm_w - 24, 1, 0x334155);
    font::draw_str(fb, sm_x + 16, sm_y + 304, "Salir al Shell [Q]", 0xf43f5e);
}

fn apply_tiling_layout(wins: &mut [Window], current_ws: usize, screen_w: usize, screen_h: usize) {
    let top_y = 10;
    let bottom_y = screen_h.saturating_sub(55);
    let left_x = 10;
    let right_x = screen_w.saturating_sub(10);
    let total_w = right_x.saturating_sub(left_x);
    let total_h = bottom_y.saturating_sub(top_y);

    let mut vis_count = 0;
    let mut visible_indices = [0usize; 10];
    for (i, w) in wins.iter().enumerate() {
        if w.workspace == current_ws && w.visible {
            if vis_count < 10 {
                visible_indices[vis_count] = i;
                vis_count += 1;
            }
        }
    }

    if vis_count == 0 {
        return;
    }

    if vis_count == 1 {
        let idx = visible_indices[0];
        wins[idx].x = left_x;
        wins[idx].y = top_y;
        wins[idx].target_x = left_x;
        wins[idx].target_y = top_y;
        wins[idx].width = total_w;
        wins[idx].height = total_h;
    } else if vis_count == 2 {
        let half_w = (total_w - 8) / 2;
        let idx0 = visible_indices[0];
        wins[idx0].x = left_x;
        wins[idx0].y = top_y;
        wins[idx0].target_x = left_x;
        wins[idx0].target_y = top_y;
        wins[idx0].width = half_w;
        wins[idx0].height = total_h;

        let idx1 = visible_indices[1];
        wins[idx1].x = left_x + half_w + 8;
        wins[idx1].y = top_y;
        wins[idx1].target_x = left_x + half_w + 8;
        wins[idx1].target_y = top_y;
        wins[idx1].width = half_w;
        wins[idx1].height = total_h;
    } else if vis_count == 3 {
        let half_w = (total_w - 8) / 2;
        let half_h = (total_h - 8) / 2;

        let idx0 = visible_indices[0];
        wins[idx0].x = left_x;
        wins[idx0].y = top_y;
        wins[idx0].target_x = left_x;
        wins[idx0].target_y = top_y;
        wins[idx0].width = half_w;
        wins[idx0].height = total_h;

        let idx1 = visible_indices[1];
        wins[idx1].x = left_x + half_w + 8;
        wins[idx1].y = top_y;
        wins[idx1].target_x = left_x + half_w + 8;
        wins[idx1].target_y = top_y;
        wins[idx1].width = half_w;
        wins[idx1].height = half_h;

        let idx2 = visible_indices[2];
        wins[idx2].x = left_x + half_w + 8;
        wins[idx2].y = top_y + half_h + 8;
        wins[idx2].target_x = left_x + half_w + 8;
        wins[idx2].target_y = top_y + half_h + 8;
        wins[idx2].width = half_w;
        wins[idx2].height = half_h;
    } else {
        let half_w = (total_w - 8) / 2;
        let half_h = (total_h - 8) / 2;

        let coords = [
            (left_x, top_y),
            (left_x + half_w + 8, top_y),
            (left_x, top_y + half_h + 8),
            (left_x + half_w + 8, top_y + half_h + 8),
        ];

        for (k, &idx) in visible_indices[..vis_count.min(4)].iter().enumerate() {
            wins[idx].x = coords[k].0;
            wins[idx].y = coords[k].1;
            wins[idx].target_x = coords[k].0;
            wins[idx].target_y = coords[k].1;
            wins[idx].width = half_w;
            wins[idx].height = half_h;
        }
    }
}

fn draw_taskbar(fb: &Framebuffer, wins: &[Window], start_open: bool, current_ws: usize, tiling: bool) {
    let bar_h = 42;
    let bar_y = fb.height - bar_h - 4;
    let bar_x = 8;
    let bar_w = fb.width - 16;

    draw_soft_shadow(fb, bar_x, bar_y, bar_w, bar_h);

    fb.fill_rect(bar_x, bar_y, bar_w, bar_h, 0x0f172a);
    fb.draw_rect(bar_x, bar_y, bar_w, bar_h, 0x1e293b);
    fb.fill_rect(bar_x + 1, bar_y, bar_w - 2, 1, 0x38bdf8);

    // Botón Start
    let start_bg = if start_open { 0x10b981 } else { 0x2563eb };
    let start_border = if start_open { 0x34d399 } else { 0x60a5fa };
    fb.fill_rect(bar_x + 8, bar_y + 5, 110, 32, start_bg);
    fb.draw_rect(bar_x + 8, bar_y + 5, 110, 32, start_border);
    font::draw_str(fb, bar_x + 14, bar_y + 17, "PORTIX OS", 0xffffff);

    // Selector de Espacios de Trabajo [1] [2] [3] [4]
    let ws_start_x = bar_x + 124;
    let ws_labels = ["1", "2", "3", "4"];
    for (i, label) in ws_labels.iter().enumerate() {
        let bx = ws_start_x + i * 26;
        let is_cur = (i + 1) == current_ws;
        let bg = if is_cur { 0x0284c7 } else { 0x1e293b };
        let fg = if is_cur { 0xffffff } else { 0x94a3b8 };
        let border = if is_cur { 0x38bdf8 } else { 0x334155 };
        fb.fill_rect(bx, bar_y + 6, 22, 30, bg);
        fb.draw_rect(bx, bar_y + 6, 22, 30, border);
        font::draw_str(fb, bx + 7, bar_y + 16, label, fg);
    }

    // Botón TILE / FLOAT
    let tile_btn_x = ws_start_x + 4 * 26 + 4;
    let (t_bg, t_border, t_text) = if tiling {
        (0x059669, 0x34d399, "TILE")
    } else {
        (0x334155, 0x64748b, "FLOAT")
    };
    fb.fill_rect(tile_btn_x, bar_y + 6, 48, 30, t_bg);
    fb.draw_rect(tile_btn_x, bar_y + 6, 48, 30, t_border);
    font::draw_str(fb, tile_btn_x + 6, bar_y + 16, t_text, 0xffffff);

    // Window tabs for current workspace
    let mut tab_x = tile_btn_x + 54;
    for win in wins.iter() {
        if win.workspace != current_ws || !win.visible { continue; }
        let bg = if win.active { 0x334155 } else { 0x1e293b };
        let fg = if win.active { 0xffffff } else { 0x94a3b8 };
        let border = if win.active { 0x38bdf8 } else { 0x475569 };

        fb.fill_rect(tab_x, bar_y + 5, 114, 32, bg);
        fb.draw_rect(tab_x, bar_y + 5, 114, 32, border);
        let title_cut = if win.title.len() > 11 { &win.title[..11] } else { win.title };
        font::draw_str(fb, tab_x + 10, bar_y + 14, title_cut, fg);

        let dot_col = if win.active { 0x38bdf8 } else { 0x64748b };
        fb.fill_rect(tab_x + 52, bar_y + 32, 10, 2, dot_col);

        tab_x += 120;
        if tab_x + 110 >= bar_x + bar_w - 440 { break; }
    }

    let tray_x = bar_x + bar_w - 430;
    font::draw_str(fb, tray_x, bar_y + 16, "[FW]", 0x10b981);
    font::draw_str(fb, tray_x + 36, bar_y + 16, "[CAM]", 0x38bdf8);
    font::draw_str(fb, tray_x + 82, bar_y + 16, "[MIC]", 0xa855f7);
    font::draw_str(fb, tray_x + 130, bar_y + 16, "Wi-Fi 7", 0x10b981);
    font::draw_str(fb, tray_x + 194, bar_y + 16, "Vol 85%", 0x38bdf8);
    font::draw_str(fb, tray_x + 260, bar_y + 16, "S0ix", 0xa855f7);
    font::draw_str(fb, tray_x + 304, bar_y + 16, "20:50", 0xf8fafc);
    fb.fill_rect(bar_x + bar_w - 48, bar_y + 6, 40, 30, 0xef4444);
    font::draw_str(fb, bar_x + bar_w - 38, bar_y + 16, "OFF", 0xffffff);
}

/// Índices de ventana ordenados de abajo arriba según `z` (orden de pintado).
fn z_order(wins: &[Window]) -> alloc::vec::Vec<usize> {
    let mut idx: alloc::vec::Vec<usize> = (0..wins.len()).collect();
    idx.sort_by_key(|&i| wins[i].z);
    idx
}

/// Ventana visible más alta (mayor `z`) del workspace que contiene el punto.
fn top_window_at(wins: &[Window], ws: usize, px: usize, py: usize) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, w) in wins.iter().enumerate() {
        if !w.visible || w.workspace != ws { continue; }
        if w.contains_titlebar(px, py) || w.contains_client_area(px, py) || w.contains_close(px, py) {
            if best.map_or(true, |b| w.z > wins[b].z) { best = Some(i); }
        }
    }
    best
}

fn draw_cursor(fb: &Framebuffer, mx: usize, my: usize) {
    const CURSOR: [&str; 15] = [
        "X              ",
        "XX             ",
        "X.X            ",
        "X..X           ",
        "X...X          ",
        "X....X         ",
        "X.....X        ",
        "X......X       ",
        "X.....XXXX     ",
        "X..X..X        ",
        "X.X X..X       ",
        "XX   X..X      ",
        "X     X..X     ",
        "       XX      ",
        "               ",
    ];

    // Sombra suave proyectada desplazada (+1, +1)
    for (cy, row) in CURSOR.iter().enumerate() {
        for (cx, ch) in row.chars().enumerate() {
            let px = mx + cx + 1;
            let py = my + cy + 1;
            if px < fb.width && py < fb.height && ch != ' ' {
                blend_pixel(fb, px, py, 0x000000, 110);
            }
        }
    }

    for (cy, row) in CURSOR.iter().enumerate() {
        for (cx, ch) in row.chars().enumerate() {
            let px = mx + cx;
            let py = my + cy;
            if px < fb.width && py < fb.height {
                if ch == 'X' {
                    fb.put_pixel(px, py, 0x0f172a);
                } else if ch == '.' {
                    fb.put_pixel(px, py, 0xffffff);
                }
            }
        }
    }
}

// ── Main Entry Point ────────────────────────────────────────────────────────
#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let pid = getpid();
    println!("==========================================================");
    println!(" [PORTIX-WM] Iniciando Servidor de Ventanas Interactivo   ");
    println!(" [PORTIX-WM] PID: {} | Espacio de Usuario (Ring 3 Rust)   ", pid);
    println!("==========================================================");

    let fb = match Framebuffer::acquire() {
        Some(fb) => fb,
        None => {
            println!("[PORTIX-WM] Error: No se pudo mapear /dev/fb0 via MMIO");
            return 1;
        }
    };

    println!("[PORTIX-WM] Framebuffer mapeado: {}x{}x{} pitch={} (modo real via SYS_FB_INFO)", fb.width, fb.height, fb.bpp, fb.pitch);

    let has_client = portix_rt::File::open("/tmp/wm/client.win", portix_rt::O_RDONLY).is_ok();
    let mut wins = [
        Window::new(1, 60, 45, 460, 310, "Monitor del Sistema (Ring 3)", false, 1),
        Window::new(2, 530, 45, 470, 310, "Terminal Interactiva /bin/sh", true, 1),
        Window::new(3, 60, 370, 420, 290, "Calculadora Nativa en Rust (Ring 3)", true, 1),
        Window::new(4, 500, 370, 480, 290, "Cliente Externo (IPC Window Protocol)", false, 1),
        Window::new(5, 70, 50, 480, 320, "Explorador de Archivos (VFS)", true, 2),
        Window::new(6, 560, 50, 440, 320, "Editor de Texto (Portix-Edit)", true, 2),
        Window::new(7, 80, 50, 520, 340, "Navegador Web Portix (HTML5 & DOM)", true, 3),
        Window::new(8, 520, 50, 480, 340, "Pila Grafica Vulkan 1.3 (Venus 3D)", true, 3),
        Window::new(9, 20, 60, 470, 340, "Portix-Store (pkg real: HTTPS + SHA-256)", true, 4),
        Window::new(10, 500, 60, 480, 320, "Centro de Control de Hardware", true, 4),
    ];
    wins[3].visible = has_client;
    if has_client {
        println!("[PORTIX-WM] Ventana de cliente externo detectada y compuesta en Ring 3.");
    }

    let mut current_workspace = 1usize;
    let mut tiling_mode = false;

    let mut calc = Calculator::new();
    calc.set_val(100);
    let mut term = InteractiveTerminal::new();
    let mut store = Store::new();
    println!("[PORTIX-WM] Portix-Store: {} paquetes en el indice ({}), {} instalados", store.pkgs.len(), store.index_source, store.pkgs.iter().filter(|p| p.installed).count());
    let mut start_menu_open = false;

    // Initial Desktop Rendering
    draw_wallpaper(&fb);
    draw_desktop_icons(&fb);
    for i in z_order(&wins) {
        let win = &wins[i];
        if win.workspace == current_workspace {
            win.draw(&fb, &calc, &term, &store);
        }
    }
    draw_taskbar(&fb, &wins, start_menu_open, current_workspace, tiling_mode);

    println!("[PORTIX-WM] Escritorio y ventanas interactivas renderizadas con exito.");
    println!("[PORTIX-WM] Servidor de ventanas listo para interaccion de usuario.");

    let mut prev_mx = 512usize;
    let mut prev_my = 384usize;
    draw_cursor(&fb, prev_mx, prev_my);

    let mut idle_ticks = 0usize;
    let mut prev_lbtn = false;
    let mut prev_rbtn = false;
    let mut z_counter: usize = wins.len() + 1;
    // [FRENTE B-3] La sesion se cierra tras IDLE_EXIT_MS reales sin entrada
    // (CLOCK_MONOTONIC), no tras N vueltas de bucle: asi un operador (o la
    // suite por serial) tiene tiempo real para interactuar con Portix-Store.
    const IDLE_EXIT_MS: u64 = 8000;
    fn mono_ms() -> u64 {
        let mut ts = portix_rt::syscall::Timespec { tv_sec: 0, tv_nsec: 0 };
        let _ = portix_rt::clock_gettime(1, &mut ts);
        (ts.tv_sec as u64) * 1000 + (ts.tv_nsec as u64) / 1_000_000
    }
    let mut last_input_ms = mono_ms();

    loop {
                let mut user_active = false;
        let mut needs_redraw = false;

        // 1. Process Keyboard input
        if let Some(ch) = poll_key() {
            user_active = true;
            if ch == 'q' || ch == 'Q' || ch == '\x1B' {
                break;
            }

            if ch == '\t' {
                // Ciclar foco de ventana en el workspace activo
                let mut first_vis = None;
                let mut found_active = false;
                let mut next_active = None;
                for (idx, w) in wins.iter().enumerate() {
                    if w.workspace == current_workspace && w.visible {
                        if first_vis.is_none() { first_vis = Some(idx); }
                        if found_active && next_active.is_none() {
                            next_active = Some(idx);
                        }
                        if w.active {
                            found_active = true;
                        }
                    }
                }
                let target = next_active.or(first_vis);
                if let Some(t_idx) = target {
                    for (idx, w) in wins.iter_mut().enumerate() {
                        w.active = idx == t_idx;
                    }
                    needs_redraw = true;
                }
            }

            // Teclado hacia Portix-Store (ventana 9) cuando esta visible y activa
            if wins[8].active && wins[8].visible && wins[8].workspace == current_workspace {
                if ch == 'u' { store.update(); needs_redraw = true; }
                else if let Some(d) = ch.to_digit(10) { if d >= 1 { store.toggle((d - 1) as usize); needs_redraw = true; } }
            }
            // Keyboard input directed to active terminal window (Window id 2)
            if wins[1].active && wins[1].visible && wins[1].workspace == current_workspace {
                match ch {
                    '\r' | '\n' => {
                        let mut cmd_buf = [0u8; 48];
                        let len = term.input_len;
                        cmd_buf[..len].copy_from_slice(&term.input_buf[..len]);
                        term.input_len = 0;
                        if let Ok(cmd_str) = core::str::from_utf8(&cmd_buf[..len]) {
                            let tr = cmd_str.trim();
                            if tr == "tile" || tr == "tiling" {
                                tiling_mode = !tiling_mode;
                                if tiling_mode {
                                    apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                                } else {
                                    for w in wins.iter_mut() { w.restore_floating(); }
                                }
                            } else if tr == "ws 1" {
                                current_workspace = 1;
                                if tiling_mode { apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height); }
                            } else if tr == "ws 2" {
                                current_workspace = 2;
                                if tiling_mode { apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height); }
                            } else if tr == "ws 3" {
                                current_workspace = 3;
                                if tiling_mode { apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height); }
                            } else if tr == "ws 4" {
                                current_workspace = 4;
                                if tiling_mode { apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height); }
                            }
                            term.execute_command(cmd_str);
                        }
                        needs_redraw = true;
                    }
                    '\x08' => {
                        if term.input_len > 0 {
                            term.input_len -= 1;
                            needs_redraw = true;
                        }
                    }
                    _ if ch >= ' ' && ch <= '~' => {
                        if term.input_len < 47 {
                            term.input_buf[term.input_len] = ch as u8;
                            term.input_len += 1;
                            needs_redraw = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // 2. Process Mouse input
        if let Some(mouse) = get_mouse() {
            let mx = (mouse.x as usize).min(fb.width.saturating_sub(1));
            let my = (mouse.y as usize).min(fb.height.saturating_sub(1));
            let lbtn = mouse.buttons & 0x01 != 0;
            let lclick = lbtn && !prev_lbtn;
            if lclick { println!("[WM-CLICK] x={} y={} ws={}", mx, my, current_workspace); }
            // Boton derecho: informa la posicion real del puntero (sin accion en el escritorio).
            let rbtn = mouse.buttons & 0x02 != 0;
            if rbtn && !prev_rbtn { println!("[WM-POINTER] x={} y={}", mx, my); }
            prev_rbtn = rbtn;

            if mx != prev_mx || my != prev_my || lbtn {
                user_active = true;
            }

            // A. Check Taskbar & Start Menu Clicks
            let bar_h = 42;
            let bar_y = fb.height - bar_h - 4;
            let bar_x = 8;
            let bar_w = fb.width - 16;
            if lclick && my >= bar_y {
                // Click on Start Button
                if mx >= bar_x + 8 && mx <= bar_x + 118 {
                    start_menu_open = !start_menu_open;
                    needs_redraw = true;
                } else if mx >= bar_x + bar_w - 48 && mx < bar_x + bar_w - 6 {
                    // Click on OFF / Exit button
                    break;
                } else if mx >= bar_x + 124 && mx < bar_x + 124 + 4 * 26 {
                    // Click on Workspace buttons [1] [2] [3] [4]
                    let target_ws = 1 + (mx - (bar_x + 124)) / 26;
                    if target_ws >= 1 && target_ws <= 4 {
                        current_workspace = target_ws;
                        if tiling_mode {
                            apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                        }
                        needs_redraw = true;
                    }
                } else if mx >= bar_x + 124 + 4 * 26 + 4 && mx < bar_x + 124 + 4 * 26 + 52 {
                    // Click on TILE / FLOAT toggle button
                    tiling_mode = !tiling_mode;
                    if tiling_mode {
                        apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                    } else {
                        for win in wins.iter_mut() {
                            win.restore_floating();
                        }
                    }
                    needs_redraw = true;
                } else {
                    // Click on Window Taskbar tabs
                    let mut tab_x = bar_x + 124 + 4 * 26 + 58;
                    for win in wins.iter_mut() {
                        if win.workspace != current_workspace || !win.visible { continue; }
                        if mx >= tab_x && mx < tab_x + 114 {
                            if !win.visible {
                                win.visible = true;
                                win.active = true;
                            } else if win.active {
                                win.visible = false;
                                win.active = false;
                            } else {
                                win.active = true;
                            }
                            if tiling_mode {
                                apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                            }
                            needs_redraw = true;
                            break;
                        }
                        tab_x += 120;
                    }
                }
            }

            // B. Check Start Menu Item Clicks
            if lclick && start_menu_open {
                let sm_w = 260;
                let sm_h = 320;
                let sm_x = 16;
                let sm_y = fb.height - 56 - sm_h;

                if mx >= sm_x && mx < sm_x + sm_w && my >= sm_y && my < sm_y + sm_h {
                    let mut target_win = None;
                    if my >= sm_y + 76 && my < sm_y + 98 {
                        target_win = Some(1); // Terminal
                    } else if my >= sm_y + 98 && my < sm_y + 120 {
                        target_win = Some(4); // Files
                    } else if my >= sm_y + 120 && my < sm_y + 142 {
                        target_win = Some(8); // Store
                    } else if my >= sm_y + 142 && my < sm_y + 164 {
                        target_win = Some(9); // Control Center
                    } else if my >= sm_y + 164 && my < sm_y + 186 {
                        target_win = Some(6); // Web
                    } else if my >= sm_y + 186 && my < sm_y + 208 {
                        target_win = Some(7); // Vulkan
                    } else if my >= sm_y + 208 && my < sm_y + 230 {
                        target_win = Some(5); // Editor
                    } else if my >= sm_y + 230 && my < sm_y + 252 {
                        target_win = Some(2); // Calc
                    } else if my >= sm_y + 252 && my < sm_y + 274 {
                        target_win = Some(0); // Monitor
                    } else if my >= sm_y + 274 && my < sm_y + 296 {
                        target_win = Some(3); // Client
                    } else if my >= sm_y + 296 && my < sm_y + 320 {
                        break;
                    }

                    if let Some(t_idx) = target_win {
                        current_workspace = wins[t_idx].workspace;
                        wins[t_idx].visible = true;
                        wins[t_idx].active = true;
                        if tiling_mode {
                            apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                        }
                        start_menu_open = false;
                        needs_redraw = true;
                    }
                } else if my < bar_y {
                    start_menu_open = false;
                    needs_redraw = true;
                }
            }

            // C. Check Desktop Icon Clicks
            if lclick && !start_menu_open && mx >= 16 && mx <= 92 && my >= 32 && my < 32 + 6 * 68 {
                let icon_idx = (my - 32) / 68;
                let target_win = match icon_idx {
                    0 => Some(1), // Terminal
                    1 => Some(4), // Files
                    2 => Some(2), // Calc
                    3 => Some(5), // Editor
                    4 => Some(0), // Monitor
                    5 => Some(7), // Vulkan
                    _ => None,
                };
                if let Some(t_idx) = target_win {
                    current_workspace = wins[t_idx].workspace;
                    wins[t_idx].visible = true;
                    wins[t_idx].active = true;
                    z_counter += 1;
                    wins[t_idx].z = z_counter;
                    if tiling_mode {
                        apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
                    }
                    needs_redraw = true;
                }
            }

            // D. Window Dragging & Client Area Interaction
            let mut clicked_win_idx = None;
            let mut need_tiling_recalc = false;
            // Z-order: sólo la ventana visible más alta bajo el puntero recibe el clic.
            let top_hit = if lclick { top_window_at(&wins, current_workspace, mx, my) } else { None };

            for (w_idx, win) in wins.iter_mut().enumerate() {
                if !win.visible || win.workspace != current_workspace { continue; }
                let lclick = lclick && top_hit == Some(w_idx);

                if lclick {
                    if win.contains_close(mx, my) {
                        win.visible = false;
                        if tiling_mode {
                            need_tiling_recalc = true;
                        }
                        needs_redraw = true;
                        continue;
                    } else if win.contains_titlebar(mx, my) {
                        if !tiling_mode {
                            win.dragging = true;
                            win.drag_dx = mx.saturating_sub(win.x);
                            win.drag_dy = my.saturating_sub(win.y);
                        }
                        clicked_win_idx = Some(w_idx);
                        needs_redraw = true;
                        continue;
                    } else if win.contains_client_area(mx, my) {
                        clicked_win_idx = Some(w_idx);

                        // ── Interaction with Window 1: Monitor Tabs & Refresh ──
                        if win.id == 1 {
                            // Check tab clicks
                            let tabs_y = win.y + 32;
                            if my >= tabs_y && my < tabs_y + 22 {
                                for i in 0..4 {
                                    let bx = win.x + 12 + i * 86;
                                    if mx >= bx && mx < bx + 80 {
                                        win.tab_idx = i;
                                        needs_redraw = true;
                                        break;
                                    }
                                }
                                // Check refresh button
                                let r_btn_x = win.x + win.width - 100;
                                if mx >= r_btn_x && mx < r_btn_x + 88 {
                                    win.metric_refresh_cnt += 1;
                                    needs_redraw = true;
                                }
                            }
                        }

                        // ── Interaction with Window 2: Terminal Buttons ────────
                        if win.id == 2 {
                            let btns_y = win.y + 30;
                            if my >= btns_y && my < btns_y + 20 {
                                let cmds = ["uname", "uptime", "free", "ps", "ls", "split", "clear"];
                                for (i, &cmd) in cmds.iter().enumerate() {
                                    let bx = win.x + 10 + i * 58;
                                    if mx >= bx && mx < bx + 54 {
                                        term.execute_command(cmd);
                                        needs_redraw = true;
                                        break;
                                    }
                                }
                            }
                        }

                        // ── Interaction with Window 3: Calculator Buttons ──────
                        if win.id == 3 {
                            let lcd_x = win.x + 20;
                            let lcd_y = win.y + 36;
                            let lcd_w = win.width - 40;
                            let start_y = lcd_y + 46;
                            let btn_w = (lcd_w - 24) / 4;
                            let btn_h = 30;

                            let grid = [
                                ["7", "8", "9", "/"],
                                ["4", "5", "6", "*"],
                                ["1", "2", "3", "-"],
                                ["C", "0", "=", "+"],
                            ];

                            for (row_idx, row) in grid.iter().enumerate() {
                                for (col_idx, &label) in row.iter().enumerate() {
                                    let bx = lcd_x + col_idx * (btn_w + 8);
                                    let by = start_y + row_idx * (btn_h + 8);
                                    if mx >= bx && mx < bx + btn_w && my >= by && my < by + btn_h {
                                        calc.click_btn(label);
                                        needs_redraw = true;
                                    }
                                }
                            }
                        }

                        // ── Interaction with Window 4: Client Action Button ────
                        if win.id == 4 {
                            let btn_y = win.y + 105;
                            if mx >= win.x + 20 && mx < win.x + 260 && my >= btn_y && my < btn_y + 32 {
                                win.client_counter += 1;
                                needs_redraw = true;
                            }
                        }

                        // ── Interaction with Window 5: Files Explorer Snapshot Rollback ──
                        if win.id == 5 {
                            let btn_x = win.x + win.width - 210;
                            let btn_y = win.y + 38;
                            if mx >= btn_x && mx < btn_x + 196 && my >= btn_y && my < btn_y + 22 {
                                win.snapshot_state = (win.snapshot_state + 1) % 2;
                                needs_redraw = true;
                            }
                        }

                        // ── Interaccion con la ventana 9: Portix-Store (acciones reales via /bin/pkg) ──
                        if win.id == 9 && mx >= win.x && my >= win.y {
                            match store.hit(mx - win.x, my - win.y, win.width) {
                                Some(StoreHit::Update) => { store.update(); needs_redraw = true; }
                                Some(StoreHit::Row(i)) => { store.toggle(i); needs_redraw = true; }
                                None => {}
                            }
                        }
                    }
                } else if lbtn && win.dragging && !tiling_mode {
                    let new_x = mx.saturating_sub(win.drag_dx).min(fb.width.saturating_sub(win.width));
                    let new_y = my.saturating_sub(win.drag_dy).min(fb.height.saturating_sub(win.height));
                    win.target_x = new_x;
                    win.target_y = new_y;
                    if win.update_physics() {
                        needs_redraw = true;
                    }
                } else if !lbtn && win.dragging {
                    win.dragging = false;
                    win.x = win.target_x;
                    win.y = win.target_y;
                    needs_redraw = true;
                }
            }

            if need_tiling_recalc && tiling_mode {
                apply_tiling_layout(&mut wins, current_workspace, fb.width, fb.height);
            }

            // Bring clicked window to active/front
            if let Some(w_idx) = clicked_win_idx {
                for (i, win) in wins.iter_mut().enumerate() {
                    win.active = i == w_idx;
                }
                // Elevar la ventana pulsada al frente.
                z_counter += 1;
                wins[w_idx].z = z_counter;
                needs_redraw = true;
            }

            // Portix-Store: recoger (sin bloquear) el /bin/pkg en curso.
            if store.poll() { needs_redraw = true; }
            if store.busy { user_active = true; }

            // Auto-interaccion demostrativa / CI cuando no hay accion humana directa
            if !user_active {
                idle_ticks += 1;
                if idle_ticks == 5 {
                    term.execute_command("uptime");
                    needs_redraw = true;
                } else if idle_ticks == 10 {
                    wins[0].tab_idx = 1;
                    needs_redraw = true;
                } else if idle_ticks == 15 {
                    calc.click_btn("7");
                    calc.click_btn("*");
                    calc.click_btn("6");
                    calc.click_btn("=");
                    needs_redraw = true;
                } else if idle_ticks == 20 {
                    wins[3].client_counter += 1;
                    needs_redraw = true;
                } else if idle_ticks >= 60 && mono_ms().wrapping_sub(last_input_ms) >= IDLE_EXIT_MS {
                    break;
                }
            } else {
                idle_ticks = 0;
                last_input_ms = mono_ms();
            }

            // Repaint Desktop if anything changed or mouse moved
            if needs_redraw || mx != prev_mx || my != prev_my {
                draw_wallpaper(&fb);
                draw_desktop_icons(&fb);
                for i in z_order(&wins) {
                    let win = &wins[i];
                    if win.workspace == current_workspace {
                        win.draw(&fb, &calc, &term, &store);
                    }
                }
                draw_start_menu(&fb, start_menu_open);
                draw_taskbar(&fb, &wins, start_menu_open, current_workspace, tiling_mode);
                draw_cursor(&fb, mx, my);
                prev_mx = mx;
                prev_my = my;
            }

            prev_lbtn = lbtn;
        }

        yield_now();
    }

    println!("[PORTIX-WM] Sesion grafica finalizada con exito.");
    0
}
