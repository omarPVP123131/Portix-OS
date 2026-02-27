// kernel/src/console/terminal.rs
//
// CAMBIOS vs v6:
//   - Comandos en INGLÉS (nombres originales: help, cpu, mem, disks, pci, etc.)
//     con aliases en español donde tiene sentido.
//   - TODA la salida, mensajes de error y descripciones en ESPAÑOL.
//   - visible_range() y line_at() corregidos (ring-buffer).
//   - Scroll robusto con max_scroll() correcto.
#![allow(dead_code)]

pub const TERM_COLS: usize = 92;
pub const TERM_ROWS: usize = 128;
pub const INPUT_MAX: usize = 80;
pub const PROMPT:    &[u8] = b"PORTIX> ";
pub const SCROLL_STEP: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineColor { Normal, Success, Warning, Error, Info, Prompt, Header }

#[derive(Clone, Copy)]
pub struct TermLine {
    pub buf:   [u8; TERM_COLS],
    pub len:   usize,
    pub color: LineColor,
}
impl TermLine {
    const fn empty() -> Self {
        TermLine { buf: [0; TERM_COLS], len: 0, color: LineColor::Normal }
    }
}

pub struct Terminal {
    pub lines:         [TermLine; TERM_ROWS],
    pub line_count:    usize,
    pub input:         [u8; INPUT_MAX],
    pub input_len:     usize,
    pub cursor_vis:    bool,
    pub scroll_offset: usize,

    hist_cmds:  [[u8; INPUT_MAX]; 16],
    hist_lens:  [usize; 16],
    hist_count: usize,
}

impl Terminal {
    pub const fn new() -> Self {
        Terminal {
            lines:         [TermLine::empty(); TERM_ROWS],
            line_count:    0,
            input:         [0u8; INPUT_MAX],
            input_len:     0,
            cursor_vis:    true,
            scroll_offset: 0,
            hist_cmds:     [[0u8; INPUT_MAX]; 16],
            hist_lens:     [0usize; 16],
            hist_count:    0,
        }
    }

    // ══ Escritura ══════════════════════════════════════════════════════════════

    pub fn write_line(&mut self, s: &str, color: LineColor) {
        self.write_bytes(s.as_bytes(), color);
    }

    pub fn write_bytes(&mut self, s: &[u8], color: LineColor) {
        let mut start = 0;
        loop {
            let end   = (start + TERM_COLS).min(s.len());
            let chunk = &s[start..end];
            let row   = self.line_count % TERM_ROWS;
            let len   = chunk.len();
            self.lines[row].buf[..len].copy_from_slice(chunk);
            for b in &mut self.lines[row].buf[len..] { *b = 0; }
            self.lines[row].len   = len;
            self.lines[row].color = color;
            self.line_count += 1;
            start = end;
            if start >= s.len() { break; }
        }
        self.scroll_offset = 0;
    }

    pub fn write_empty(&mut self) { self.write_bytes(b"", LineColor::Normal); }

    // ══ Ring-buffer ════════════════════════════════════════════════════════════

    /// Acceso a línea por índice lógico (0 = más antigua disponible).
    #[inline]
    pub fn line_at(&self, li: usize) -> &TermLine {
        &self.lines[li % TERM_ROWS]
    }

    #[inline]
    fn oldest_logical(&self) -> usize {
        if self.line_count <= TERM_ROWS { 0 } else { self.line_count - TERM_ROWS }
    }

    /// Máximo scroll_offset posible dada la ventana visible.
    pub fn max_scroll(&self, max_visible: usize) -> usize {
        let available = self.line_count.saturating_sub(self.oldest_logical());
        available.saturating_sub(max_visible)
    }

    // ══ Scroll ═════════════════════════════════════════════════════════════════

    pub fn scroll_up(&mut self, lines: usize, max_visible: usize) {
        let max = self.max_scroll(max_visible);
        self.scroll_offset = (self.scroll_offset + lines).min(max);
    }
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }
    pub fn scroll_to_bottom(&mut self) { self.scroll_offset = 0; }
    pub fn at_bottom(&self) -> bool    { self.scroll_offset == 0 }

    /// Retorna (inicio_lógico, cantidad) para el render.
    /// Usar: `for i in 0..count { let line = term.line_at(start + i); ... }`
    pub fn visible_range(&self, max_visible: usize) -> (usize, usize) {
        if self.line_count == 0 { return (0, 0); }
        let oldest          = self.oldest_logical();
        let total_available = self.line_count - oldest;
        let count           = total_available.min(max_visible);
        let bottom_start    = self.line_count.saturating_sub(count);
        let start           = bottom_start.saturating_sub(self.scroll_offset).max(oldest);
        let end             = (start + count).min(self.line_count);
        (start, end.saturating_sub(start))
    }

    // ══ Input ═══════════════════════════════════════════════════════════════════

    pub fn type_char(&mut self, c: u8) {
        if self.input_len < INPUT_MAX - 1 && c >= 32 && c < 127 {
            self.input[self.input_len] = c; self.input_len += 1;
        }
    }
    pub fn backspace(&mut self) { if self.input_len > 0 { self.input_len -= 1; } }
    pub fn clear_input(&mut self) {
        self.input_len = 0; for b in &mut self.input { *b = 0; }
    }
    pub fn clear_history(&mut self) {
        for l in &mut self.lines { l.len = 0; l.buf[0] = 0; }
        self.line_count = 0; self.scroll_offset = 0;
    }

    // ══ Enter ═══════════════════════════════════════════════════════════════════

    pub fn enter(&mut self, hw: &crate::arch::hardware::HardwareInfo, pci: &crate::drivers::bus::pci::PciBus) {
        let mut echo = [0u8; INPUT_MAX + 10];
        let plen = PROMPT.len();
        echo[..plen].copy_from_slice(PROMPT);
        echo[plen..plen + self.input_len].copy_from_slice(&self.input[..self.input_len]);
        self.write_bytes(&echo[..plen + self.input_len], LineColor::Prompt);

        if self.input_len > 0 {
            let slot = self.hist_count % 16;
            self.hist_cmds[slot][..self.input_len].copy_from_slice(&self.input[..self.input_len]);
            self.hist_lens[slot] = self.input_len;
            self.hist_count += 1;
        }

        let mut cmd_buf  = [0u8; INPUT_MAX];
        let mut args_buf = [0u8; INPUT_MAX];
        let cmd_len; let args_len;
        {
            let raw     = &self.input[..self.input_len];
            let start   = raw.iter().position(|&b| b != b' ').unwrap_or(0);
            let trimmed = &raw[start..];
            let end     = trimmed.iter().rposition(|&b| b != b' ').map(|i| i+1).unwrap_or(0);
            let cmd_bytes = &trimmed[..end];
            let split = cmd_bytes.iter().position(|&b| b == b' ');
            let (cmd_tok, args) = if let Some(sp) = split {
                (&cmd_bytes[..sp], &cmd_bytes[sp+1..])
            } else { (cmd_bytes, &b""[..]) };
            cmd_len  = cmd_tok.len().min(INPUT_MAX);
            args_len = args.len().min(INPUT_MAX);
            cmd_buf [..cmd_len ].copy_from_slice(&cmd_tok[..cmd_len]);
            args_buf[..args_len].copy_from_slice(&args[..args_len]);
        }

        if cmd_len == 0 { self.clear_input(); return; }
        self.dispatch(&cmd_buf[..cmd_len], &args_buf[..args_len], hw, pci);
        self.clear_input();
    }

    fn dispatch(&mut self, cmd: &[u8], args: &[u8],
                hw: &crate::arch::hardware::HardwareInfo, pci: &crate::drivers::bus::pci::PciBus) {
        match cmd {
            // ── Ayuda ──────────────────────────────────────────────────────────
            b"help" | b"ayuda" | b"?" | b"h"
                => self.cmd_help(),

            // ── Información del sistema ────────────────────────────────────────
            b"info"
                => self.cmd_info(hw),
            b"cpu" | b"lscpu"
                => self.cmd_cpu(hw),
            b"mem" | b"memory" | b"lsmem"
                => self.cmd_mem(hw),
            b"disks" | b"storage" | b"lsblk"
                => self.cmd_disks(hw),
            b"pci" | b"lspci"
                => self.cmd_pci(pci),
            b"neofetch" | b"fetch"
                => self.cmd_neofetch(hw, pci),
            b"uname"
                => self.write_line("  PORTIX 0.7 x86_64 bare-metal #1", LineColor::Normal),
            b"whoami"
                => self.write_line("  root", LineColor::Success),
            b"hostname"
                => self.write_line("  portix-kernel", LineColor::Normal),
            b"motd"
                => self.cmd_motd(),
            b"ver" | b"version"
                => self.cmd_ver(),
            b"uptime" | b"time"
                => self.cmd_uptime(),
            b"date" | b"fecha"
                => self.cmd_fecha(),

            // ── Terminal ───────────────────────────────────────────────────────
            b"clear" | b"cls" | b"limpiar"
                => self.clear_history(),
            b"echo" | b"print"
                => self.write_bytes(args, LineColor::Normal),
            b"history" | b"historial"
                => self.cmd_history(),

            // ── Cálculo y conversión ───────────────────────────────────────────
            b"calc" | b"math" | b"="
                => self.cmd_calc(args),
            b"hex"  => self.cmd_hex(args),
            b"dec"  => self.cmd_dec(args),
            b"bin"  => self.cmd_bin(args),
            b"rgb"  => self.cmd_rgb(args),

            // ── Hardware / depuración ─────────────────────────────────────────
            b"hexdump" | b"dump" | b"hd"
                => self.cmd_hexdump(args),
            b"peek" => self.cmd_peek(args),
            b"poke" => self.cmd_poke(args),
            b"cpuid"=> self.cmd_cpuid(args),
            b"pic" | b"lsirq"
                => self.cmd_pic(),
            b"gdt"  => self.cmd_gdt(),
            b"memtest"
                => self.cmd_memtest(args),
            b"ticks"=> self.cmd_ticks(),
            b"inb"  => self.cmd_inb(args),
            b"outb" => self.cmd_outb(args),

            // ── Audio / entretenimiento ────────────────────────────────────────
            b"beep" => self.cmd_beep(args),
            b"colors" | b"palette" | b"colores"
                => self.cmd_colors(),
            b"ascii" | b"art"
                => self.cmd_ascii_art(),
            b"banner"
                => self.cmd_banner(args),
            b"progress"
                => self.cmd_progress(),
            b"matrix"
                => self.cmd_matrix(),
            b"scrolltest" | b"scroll"
                => self.cmd_scrolltest(),

            // ── Energía ───────────────────────────────────────────────────────
            b"poweroff" | b"shutdown" | b"apagar" => {
                self.write_line("  Apagando el sistema...", LineColor::Warning);
                crate::drivers::bus::acpi::poweroff();
            }
            b"reboot" | b"restart" | b"reiniciar" => {
                self.write_line("  Reiniciando...", LineColor::Warning);
                crate::drivers::bus::acpi::reboot();
            }

            // ── Comando desconocido ────────────────────────────────────────────
            _ => {
                let mut buf = [0u8; 80]; let mut pos = 0;
                for b in b"  Error: comando no encontrado: " { buf[pos]=*b; pos+=1; }
                let l = cmd.len().min(40);
                buf[pos..pos+l].copy_from_slice(&cmd[..l]);
                self.write_bytes(&buf[..pos+l], LineColor::Error);
                self.write_line("  Escribe 'help' para ver los comandos disponibles.", LineColor::Normal);
            }
        }
    }

    // ══ Helpers de presentación ════════════════════════════════════════════════

    fn separador(&mut self, titulo: &str) {
        let tb = titulo.as_bytes();
        let tl = tb.len();
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  +-- ");
        let l = tl.min(60);
        buf[pos..pos+l].copy_from_slice(&tb[..l]); pos += l;
        append_str(&mut buf, &mut pos, b" ");
        let dashes = 58usize.saturating_sub(tl);
        for _ in 0..dashes.min(TERM_COLS.saturating_sub(pos).saturating_sub(2)) {
            buf[pos] = b'-'; pos += 1;
        }
        buf[pos] = b'+'; pos += 1;
        self.write_bytes(&buf[..pos], LineColor::Header);
    }

    // ══ Comandos ═══════════════════════════════════════════════════════════════

    fn cmd_help(&mut self) {
        self.write_empty();
        self.write_line("  +=========================================================+", LineColor::Header);
        self.write_line("  |       PORTIX v0.7  -  Referencia de Comandos            |", LineColor::Header);
        self.write_line("  +=========================================================+", LineColor::Header);
        self.write_empty();

        self.write_line("  INFORMACION DEL SISTEMA:", LineColor::Info);
        self.write_line("    neofetch      Vista general del sistema con logo ASCII", LineColor::Normal);
        self.write_line("    cpu           Detalles del procesador y extensiones ISA", LineColor::Normal);
        self.write_line("    mem           Mapa de memoria RAM (E820)", LineColor::Normal);
        self.write_line("    disks         Dispositivos de almacenamiento ATA", LineColor::Normal);
        self.write_line("    pci           Enumeracion del bus PCI", LineColor::Normal);
        self.write_line("    uname / ver   Version del sistema operativo", LineColor::Normal);
        self.write_line("    uptime        Tiempo en linea y ticks del PIT", LineColor::Normal);
        self.write_line("    date          Fecha/hora desde el arranque", LineColor::Normal);
        self.write_empty();

        self.write_line("  HARDWARE Y DEPURACION:", LineColor::Info);
        self.write_line("    hexdump <dir> [bytes]  Volcado hexadecimal de memoria", LineColor::Normal);
        self.write_line("    peek <dir>             Leer 8 bytes en direccion fisica", LineColor::Normal);
        self.write_line("    poke <dir> <val>       Escribir byte en direccion fisica", LineColor::Normal);
        self.write_line("    cpuid [hoja]           Ejecutar instruccion CPUID", LineColor::Normal);
        self.write_line("    pic                    Estado de mascaras del PIC/IRQ", LineColor::Normal);
        self.write_line("    gdt                    Volcado de la tabla GDT", LineColor::Normal);
        self.write_line("    memtest [dir] [tam]    Prueba de lectura/escritura de RAM", LineColor::Normal);
        self.write_line("    inb <puerto>           Leer byte de puerto de E/S", LineColor::Normal);
        self.write_line("    outb <puerto> <val>    Escribir byte en puerto de E/S", LineColor::Normal);
        self.write_empty();

        self.write_line("  CALCULO Y CONVERSION:", LineColor::Info);
        self.write_line("    calc / = <expr>   Aritmetica: + - * /", LineColor::Normal);
        self.write_line("    hex <decimal>     Decimal a hexadecimal", LineColor::Normal);
        self.write_line("    dec <0xHEX>       Hexadecimal a decimal", LineColor::Normal);
        self.write_line("    bin <decimal>     Decimal a binario", LineColor::Normal);
        self.write_line("    rgb <r> <g> <b>   Componentes RGB a 0xRRGGBB", LineColor::Normal);
        self.write_empty();

        self.write_line("  TERMINAL:", LineColor::Info);
        self.write_line("    echo <texto>   Imprimir texto en pantalla", LineColor::Normal);
        self.write_line("    history        Historial de comandos (ultimos 16)", LineColor::Normal);
        self.write_line("    clear          Limpiar la pantalla del terminal", LineColor::Normal);
        self.write_line("    scrolltest     Generar 50 lineas para probar scroll", LineColor::Normal);
        self.write_empty();

        self.write_line("  NAVEGACION POR EL HISTORIAL:", LineColor::Info);
        self.write_line("    RePag / AvPag  Desplazarse 10 lineas arriba/abajo", LineColor::Normal);
        self.write_line("    Inicio / Fin   Saltar al principio / final", LineColor::Normal);
        self.write_line("    Mouse          Arrastrar la barra lateral para navegar", LineColor::Normal);
        self.write_empty();

        self.write_line("  AUDIO Y EFECTOS:", LineColor::Info);
        self.write_line("    beep [hz]     Pitido por el altavoz interno del PC", LineColor::Normal);
        self.write_line("    matrix        Animacion ASCII estilo Matrix", LineColor::Normal);
        self.write_line("    colors        Demostracion de paleta de colores", LineColor::Normal);
        self.write_line("    ascii         Logo ASCII de PORTIX", LineColor::Normal);
        self.write_line("    banner <txt>  Mostrar texto en formato de pancarta", LineColor::Normal);
        self.write_empty();

        self.write_line("  ENERGIA:", LineColor::Warning);
        self.write_line("    reboot        Reiniciar el sistema", LineColor::Normal);
        self.write_line("    poweroff      Apagar el sistema (ACPI S5)", LineColor::Normal);
        self.write_empty();
    }

    fn cmd_ver(&mut self) {
        self.separador("VERSION DEL SISTEMA");
        self.write_line("  PORTIX Kernel v0.7  -  x86_64 bare-metal", LineColor::Success);
        self.write_line("  Compilacion: 2026 / Rust nightly (no_std) + NASM", LineColor::Normal);
        self.write_line("  Subsistemas: PIT  Teclado PS/2  Raton PS/2  ATA  VESA  DblBuf", LineColor::Info);
        self.write_line("               PCI  ACPI  Serial COM1  E820  IDT", LineColor::Info);
        self.write_empty();
    }

    fn cmd_motd(&mut self) {
        self.write_empty();
        self.write_line("   ██████╗  ██████╗ ██████╗ ████████╗██╗██╗  ██╗", LineColor::Header);
        self.write_line("   ██╔══██╗██╔═══██╗██╔══██╗╚══██╔══╝██║╚██╗██╔╝", LineColor::Header);
        self.write_line("   ██████╔╝██║   ██║██████╔╝   ██║   ██║ ╚███╔╝ ", LineColor::Header);
        self.write_line("   ██╔═══╝ ██║   ██║██╔══██╗   ██║   ██║ ██╔██╗ ", LineColor::Header);
        self.write_line("   ██║     ╚██████╔╝██║  ██║   ██║   ██║██╔╝ ██╗", LineColor::Header);
        self.write_line("   ╚═╝      ╚═════╝ ╚═╝  ╚═╝   ╚═╝   ╚═╝╚═╝  ╚═╝", LineColor::Header);
        self.write_empty();
        self.write_line("   Sistema Operativo Bare-Metal | x86_64 | Rust + NASM | Doble Buffer", LineColor::Info);
        self.write_line("   Sin stdlib. Sin runtime. Sin piedad. Solo metal.", LineColor::Normal);
        self.write_empty();
    }

    fn cmd_ascii_art(&mut self) {
        self.write_empty();
        self.write_line("    .--------.", LineColor::Header);
        self.write_line("    | PORTIX |   .-------------.", LineColor::Header);
        self.write_line("    |  v0.7  |   |   x86_64    |", LineColor::Header);
        self.write_line("    |        |   | bare-metal   |", LineColor::Header);
        self.write_line("    '--------'   | Doble Buffer |", LineColor::Header);
        self.write_line("        ||       '-------------'", LineColor::Header);
        self.write_line("    [========]   Rust + NASM", LineColor::Info);
        self.write_line("    |        |   Sin stdlib.", LineColor::Info);
        self.write_line("    [========]", LineColor::Info);
        self.write_empty();
    }

    fn cmd_scrolltest(&mut self) {
        self.separador("PRUEBA DE SCROLL");
        self.write_line("  Generando 50 lineas... usa RePag/AvPag o arrastra la barra.", LineColor::Info);
        self.write_empty();
        for i in 0u32..50 {
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Linea #");
            append_u32(&mut buf, &mut pos, i + 1);
            append_str(&mut buf, &mut pos, b"  -- desplazate con RePag/AvPag o la barra lateral");
            let col = match i % 5 {
                0 => LineColor::Normal,  1 => LineColor::Info,
                2 => LineColor::Success, 3 => LineColor::Warning,
                _ => LineColor::Header,
            };
            self.write_bytes(&buf[..pos], col);
        }
        self.write_empty();
        self.write_line("  [OK] Fin de la prueba.", LineColor::Success);
    }

    fn cmd_matrix(&mut self) {
        self.write_empty();
        self.write_line("  Despierta, Neo...", LineColor::Success);
        let seed = crate::time::pit::ticks() as u32;
        let chars = b"01ABCDEF<>{}[]!?#$@*";
        for row in 0..8u32 {
            let mut line = [b' '; TERM_COLS]; let mut lp = 0;
            line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1;
            for col in 0..60usize {
                let v = (seed ^ (row * 31337 + col as u32 * 13)).wrapping_mul(0x6B43_9AA7) >> 24;
                let ch = if v < 180 { chars[(v as usize) % chars.len()] } else { b' ' };
                if lp < TERM_COLS - 1 { line[lp] = ch; lp += 1; }
                if lp < TERM_COLS - 1 { line[lp] = b' '; lp += 1; }
            }
            let col = if row % 3 == 0 { LineColor::Success }
                      else if row % 3 == 1 { LineColor::Info } else { LineColor::Normal };
            self.write_bytes(&line[..lp], col);
        }
        self.write_empty();
        self.write_line("  La Matrix te tiene.", LineColor::Warning);
        self.write_empty();
    }

    fn cmd_banner(&mut self, args: &[u8]) {
        if args.is_empty() {
            self.write_line("  Uso: banner <texto>  (max 8 chars)", LineColor::Warning); return;
        }
        let s = core::str::from_utf8(args).unwrap_or("?");
        let s = if s.len() > 8 { &s[..8] } else { s };
        self.write_empty();
        let w = (s.len() * 2 + 4).min(40);
        let mut top = [0u8; 80]; let mut tp = 0;
        top[tp] = b'+'; tp += 1;
        for _ in 0..w { if tp < 79 { top[tp] = b'='; tp += 1; } }
        top[tp] = b'+'; tp += 1;
        self.write_bytes(&top[..tp], LineColor::Header);
        let mut mid = [0u8; 80]; let mut mp = 0;
        mid[mp] = b'|'; mp += 1; mid[mp] = b' '; mp += 1;
        for ch in s.chars() {
            let u = if ch.is_ascii_lowercase() { ch as u8 - b'a' + b'A' } else { ch as u8 };
            if mp < 78 { mid[mp] = u; mp += 1; }
            if mp < 78 { mid[mp] = b' '; mp += 1; }
        }
        mid[mp] = b' '; mp += 1; mid[mp] = b'|'; mp += 1;
        self.write_bytes(&mid[..mp], LineColor::Success);
        let mut bot = [0u8; 80]; let mut bp = 0;
        bot[bp] = b'+'; bp += 1;
        for _ in 0..w { if bp < 79 { bot[bp] = b'='; bp += 1; } }
        bot[bp] = b'+'; bp += 1;
        self.write_bytes(&bot[..bp], LineColor::Header);
        self.write_empty();
    }

    fn cmd_colors(&mut self) {
        self.write_empty();
        self.separador("PALETA DE COLORES DEL TERMINAL");
        self.write_empty();
        self.write_line("  Normal   -- texto estandar",         LineColor::Normal);
        self.write_line("  EXITO    -- operacion correcta",     LineColor::Success);
        self.write_line("  AVISO    -- atencion no critica",    LineColor::Warning);
        self.write_line("  ERROR    -- algo fallo",             LineColor::Error);
        self.write_line("  INFO     -- informacion del sistema",LineColor::Info);
        self.write_line("  PROMPT   -- linea de comandos",      LineColor::Prompt);
        self.write_line("  CABECERA -- titulo de seccion",      LineColor::Header);
        self.write_empty();
    }

    fn cmd_progress(&mut self) {
        self.write_empty();
        self.write_line("  Cargando componentes de PORTIX:", LineColor::Info);
        for pct in [20u32, 40, 60, 80, 100] {
            let mut bar = [b' '; 52];
            bar[0] = b'[';
            let filled = (pct as usize * 50) / 100;
            for i in 0..filled { bar[1 + i] = b'#'; }
            bar[51] = b']';
            let mut line = [0u8; 80]; let mut lp = 0;
            line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1;
            for &b in &bar { if lp < 79 { line[lp] = b; lp += 1; } }
            line[lp] = b' '; lp += 1;
            append_u32(&mut line, &mut lp, pct); line[lp] = b'%'; lp += 1;
            self.write_bytes(&line[..lp], if pct==100 { LineColor::Success } else { LineColor::Info });
        }
        self.write_empty();
    }

    fn cmd_fecha(&mut self) {
        let (h, m, s) = crate::time::pit::uptime_hms();
        let t = crate::time::pit::ticks();
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Tiempo desde arranque: ");
        append_u32(&mut buf, &mut pos, h); append_str(&mut buf, &mut pos, b"h ");
        if m < 10 { append_str(&mut buf, &mut pos, b"0"); }
        append_u32(&mut buf, &mut pos, m); append_str(&mut buf, &mut pos, b"m ");
        if s < 10 { append_str(&mut buf, &mut pos, b"0"); }
        append_u32(&mut buf, &mut pos, s); append_str(&mut buf, &mut pos, b"s");
        append_str(&mut buf, &mut pos, b"  (tick #");
        append_u32(&mut buf, &mut pos, (t & 0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b")");
        self.write_bytes(&buf[..pos], LineColor::Info);
        self.write_line("  Nota: sin driver RTC -- tiempo mostrado es desde el arranque.", LineColor::Normal);
    }

    fn cmd_uptime(&mut self) {
        let (h, m, s) = crate::time::pit::uptime_hms();
        let t = crate::time::pit::ticks();
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Tiempo en linea: ");
        append_u32(&mut buf, &mut pos, h); append_str(&mut buf, &mut pos, b"h ");
        append_u32(&mut buf, &mut pos, m); append_str(&mut buf, &mut pos, b"m ");
        append_u32(&mut buf, &mut pos, s); append_str(&mut buf, &mut pos, b"s");
        append_str(&mut buf, &mut pos, b"   |  Ticks: ");
        append_u32(&mut buf, &mut pos, (t & 0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b" @ 100 Hz");
        self.write_bytes(&buf[..pos], LineColor::Success);
    }

    fn cmd_ticks(&mut self) {
        let t = crate::time::pit::ticks();
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Ticks del PIT: ");
        append_u32(&mut buf, &mut pos, (t & 0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b"  (alto 32 bits: ");
        append_u32(&mut buf, &mut pos, (t >> 32) as u32);
        append_str(&mut buf, &mut pos, b")");
        self.write_bytes(&buf[..pos], LineColor::Info);
    }

    fn cmd_history(&mut self) {
        if self.hist_count == 0 {
            self.write_line("  (sin historial de comandos)", LineColor::Normal); return;
        }
        self.separador("HISTORIAL DE COMANDOS");
        let total = self.hist_count.min(16);
        let start = if self.hist_count > 16 { self.hist_count - 16 } else { 0 };
        for i in 0..total {
            let slot = (start + i) % 16;
            let len  = self.hist_lens[slot]; if len == 0 { continue; }
            let mut line = [0u8; 80]; let mut lp = 0;
            append_str(&mut line, &mut lp, b"  ");
            append_u32(&mut line, &mut lp, (start + i + 1) as u32);
            append_str(&mut line, &mut lp, b"  ");
            let l = len.min(70);
            line[lp..lp+l].copy_from_slice(&self.hist_cmds[slot][..l]); lp += l;
            self.write_bytes(&line[..lp], LineColor::Normal);
        }
        self.write_empty();
    }

    fn cmd_info(&mut self, hw: &crate::arch::hardware::HardwareInfo) {
        self.cmd_cpu(hw); self.write_empty(); self.cmd_mem(hw);
    }

    fn cmd_cpu(&mut self, hw: &crate::arch::hardware::HardwareInfo) {
        self.separador("PROCESADOR (CPU)");
        {
            let mut lb = [0u8; TERM_COLS]; let bl = b"  Modelo     : ";
            lb[..bl.len()].copy_from_slice(bl);
            let n = hw.cpu.brand_str().as_bytes();
            let nl = n.len().min(TERM_COLS - bl.len());
            lb[bl.len()..bl.len()+nl].copy_from_slice(&n[..nl]);
            self.write_bytes(&lb[..bl.len()+nl], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Nucleos    : ");
            append_u32(&mut buf, &mut pos, hw.cpu.physical_cores as u32);
            append_str(&mut buf, &mut pos, b" fisicos / ");
            append_u32(&mut buf, &mut pos, hw.cpu.logical_cores as u32);
            append_str(&mut buf, &mut pos, b" logicos");
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Frecuencia : max ");
            append_mhz(&mut buf, &mut pos, hw.cpu.max_mhz);
            if hw.cpu.base_mhz > 0 && hw.cpu.base_mhz != hw.cpu.max_mhz {
                append_str(&mut buf, &mut pos, b"  base ");
                append_mhz(&mut buf, &mut pos, hw.cpu.base_mhz);
            }
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Extensiones:");
            if hw.cpu.has_sse2 { append_str(&mut buf, &mut pos, b" SSE2"); }
            if hw.cpu.has_sse4 { append_str(&mut buf, &mut pos, b" SSE4"); }
            if hw.cpu.has_avx  { append_str(&mut buf, &mut pos, b" AVX");  }
            if hw.cpu.has_avx2 { append_str(&mut buf, &mut pos, b" AVX2"); }
            if hw.cpu.has_aes  { append_str(&mut buf, &mut pos, b" AES");  }
            self.write_bytes(&buf[..pos], LineColor::Success);
        }
        self.write_empty();
    }

    fn cmd_mem(&mut self, hw: &crate::arch::hardware::HardwareInfo) {
        self.separador("MEMORIA RAM (E820)");
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Utilizable : ");
            append_mib(&mut buf, &mut pos, hw.ram.usable_or_default());
            append_str(&mut buf, &mut pos, b"   Entradas E820: ");
            append_u32(&mut buf, &mut pos, hw.ram.entry_count as u32);
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        self.write_empty();
        self.write_line("  #  Base                   Longitud      Tipo", LineColor::Info);
        self.write_line("  -  --------------------   -----------   ----------", LineColor::Normal);
        unsafe {
            for i in 0..(hw.ram.entry_count.min(16) as usize) {
                let p    = (0x9102usize + i * 20) as *const u8;
                let base = core::ptr::read_unaligned(p as *const u64);
                let len  = core::ptr::read_unaligned(p.add(8) as *const u64);
                let kind = core::ptr::read_unaligned(p.add(16) as *const u32);
                let ts: &[u8] = match kind {
                    1=>b"Utilizable", 2=>b"Reservada",
                    3=>b"ACPI Reclam",4=>b"ACPI NVS",
                    5=>b"RAM Mala",   _=>b"Desconocido"
                };
                let mut eb = [0u8; TERM_COLS]; let mut ep = 0;
                append_str(&mut eb, &mut ep, b"  ");
                append_u32(&mut eb, &mut ep, i as u32);
                append_str(&mut eb, &mut ep, b"  0x");
                append_hex64_full(&mut eb, &mut ep, base);
                append_str(&mut eb, &mut ep, b"  ");
                append_mib(&mut eb, &mut ep, len / (1024*1024));
                append_str(&mut eb, &mut ep, b"   ");
                eb[ep..ep+ts.len()].copy_from_slice(ts); ep += ts.len();
                self.write_bytes(&eb[..ep], if kind==1{LineColor::Success}else{LineColor::Normal});
            }
        }
        self.write_empty();
    }

    fn cmd_disks(&mut self, hw: &crate::arch::hardware::HardwareInfo) {
        self.separador("ALMACENAMIENTO (ATA)");
        if hw.disks.count == 0 {
            self.write_line("  No se detectaron unidades ATA.", LineColor::Warning);
            self.write_empty(); return;
        }
        for i in 0..hw.disks.count {
            let d = &hw.disks.drives[i];
            let bus = if d.bus==0 { b"ATA0" as &[u8] } else { b"ATA1" };
            let drv = if d.drive==0 { b"Maestro" as &[u8] } else { b"Esclavo" };
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [");
            buf[pos..pos+bus.len()].copy_from_slice(bus); pos += bus.len();
            append_str(&mut buf, &mut pos, b"-");
            buf[pos..pos+drv.len()].copy_from_slice(drv); pos += drv.len();
            append_str(&mut buf, &mut pos, if d.is_atapi { b"]  OPTICO  " } else { b"]  HDD    " });
            let m = d.model_str().as_bytes(); let ml = m.len().min(28);
            buf[pos..pos+ml].copy_from_slice(&m[..ml]); pos += ml;
            if !d.is_atapi {
                append_str(&mut buf, &mut pos, b"  ");
                append_mib(&mut buf, &mut pos, d.size_mb);
                if d.lba48 { append_str(&mut buf, &mut pos, b"  [LBA48]"); }
            }
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        self.write_empty();
    }

    fn cmd_pci(&mut self, pci: &crate::drivers::bus::pci::PciBus) {
        self.separador("BUS PCI");
        if pci.count == 0 {
            self.write_line("  No se encontraron dispositivos PCI.", LineColor::Warning);
            self.write_empty(); return;
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Se encontraron ");
            append_u32(&mut buf, &mut pos, pci.count as u32);
            append_str(&mut buf, &mut pos, b" dispositivo(s):");
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        self.write_empty();
        self.write_line("  [B:D.F]  VendorID:DevID  Fabricante           Clase", LineColor::Info);
        self.write_line("  -------  --------------  -------------------  ---------", LineColor::Normal);
        for i in 0..pci.count.min(32) {
            let d = &pci.devices[i];
            let mut lb = [0u8; TERM_COLS]; let mut lp = 0;
            append_str(&mut lb, &mut lp, b"  [");
            append_u32(&mut lb, &mut lp, d.bus as u32);
            append_str(&mut lb, &mut lp, b":");
            append_hex8_byte(&mut lb, &mut lp, d.device);
            append_str(&mut lb, &mut lp, b".");
            append_u32(&mut lb, &mut lp, d.function as u32);
            append_str(&mut lb, &mut lp, b"]  ");
            append_hex16(&mut lb, &mut lp, d.vendor_id);
            append_str(&mut lb, &mut lp, b":");
            append_hex16(&mut lb, &mut lp, d.device_id);
            append_str(&mut lb, &mut lp, b"  ");
            let vn = d.vendor_name().as_bytes();
            lb[lp..lp+vn.len()].copy_from_slice(vn); lp += vn.len();
            while lp < 56 { lb[lp] = b' '; lp += 1; }
            let cn = d.class_name().as_bytes(); let cl = cn.len().min(20);
            lb[lp..lp+cl].copy_from_slice(&cn[..cl]); lp += cl;
            self.write_bytes(&lb[..lp], LineColor::Info);
        }
        self.write_empty();
    }

    fn cmd_hexdump(&mut self, args: &[u8]) {
        let args = trim(args);
        if args.is_empty() {
            self.write_line("  Uso: hexdump <0xDIR> [bytes]  (predeterminado: 64)", LineColor::Warning);
            return;
        }
        let (addr_part, count_part) = if let Some(sp) = args.iter().position(|&b| b == b' ') {
            (&args[..sp], trim(&args[sp+1..]))
        } else { (args, &b""[..]) };
        let addr = match parse_hex(addr_part) { Some(a) => a, None => {
            self.write_line("  Error: direccion invalida (usa prefijo 0x)", LineColor::Error); return; }};
        let count = if count_part.is_empty() { 64 }
                    else { match parse_u64(count_part) { Some(n) => n.min(256) as usize, None => 64 } };
        {
            let mut hdr = [0u8; 80]; let mut hp = 0;
            append_str(&mut hdr, &mut hp, b"  Volcado 0x");
            append_hex64_short(&mut hdr, &mut hp, addr);
            append_str(&mut hdr, &mut hp, b" (");
            append_u32(&mut hdr, &mut hp, count as u32);
            append_str(&mut hdr, &mut hp, b" bytes):");
            self.write_bytes(&hdr[..hp], LineColor::Info);
        }
        self.write_line("  Offset    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  ASCII", LineColor::Header);
        for row in 0..((count + 15) / 16) {
            let base = addr + (row * 16) as u64;
            let mut line = [0u8; TERM_COLS]; let mut lp = 0;
            append_str(&mut line, &mut lp, b"  ");
            append_hex64_short(&mut line, &mut lp, base);
            append_str(&mut line, &mut lp, b"  ");
            let mut ascii_buf = [b'.'; 16];
            for col in 0..16usize {
                let idx = row * 16 + col;
                if col == 8 { append_str(&mut line, &mut lp, b" "); }
                if idx < count {
                    let byte = unsafe { core::ptr::read_volatile((base + col as u64) as *const u8) };
                    const H: &[u8] = b"0123456789ABCDEF";
                    if lp < TERM_COLS-1 { line[lp] = H[(byte>>4) as usize]; lp+=1; }
                    if lp < TERM_COLS-1 { line[lp] = H[(byte&0xF) as usize]; lp+=1; }
                    if lp < TERM_COLS-1 { line[lp] = b' '; lp+=1; }
                    ascii_buf[col] = if byte>=32 && byte<127 { byte } else { b'.' };
                } else { append_str(&mut line, &mut lp, b"   "); }
            }
            append_str(&mut line, &mut lp, b" ");
            let acnt = 16.min(count.saturating_sub(row*16));
            for &ac in &ascii_buf[..acnt] { if lp < TERM_COLS-1 { line[lp]=ac; lp+=1; } }
            self.write_bytes(&line[..lp], LineColor::Normal);
        }
    }

    fn cmd_peek(&mut self, args: &[u8]) {
        let args = trim(args);
        if args.is_empty() { self.write_line("  Uso: peek <0xDIR>", LineColor::Warning); return; }
        let addr = match parse_hex(args) { Some(a)=>a, None=>{ self.write_line("  Error: direccion invalida", LineColor::Error); return; }};
        let val = unsafe { core::ptr::read_volatile(addr as *const u64) };
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  [0x"); append_hex64_short(&mut buf, &mut pos, addr);
        append_str(&mut buf, &mut pos, b"] = 0x"); append_hex64_full(&mut buf, &mut pos, val);
        append_str(&mut buf, &mut pos, b" ("); append_u32(&mut buf, &mut pos, (val&0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b")");
        self.write_bytes(&buf[..pos], LineColor::Success);
    }

    fn cmd_poke(&mut self, args: &[u8]) {
        let args = trim(args);
        let sp = match args.iter().position(|&b| b==b' ') { Some(i)=>i, None=>{ self.write_line("  Uso: poke <0xDIR> <valor>", LineColor::Warning); return; }};
        let addr = match parse_hex(&args[..sp]) { Some(a)=>a, None=>{ self.write_line("  Error: direccion invalida", LineColor::Error); return; }};
        let val  = match parse_u64(trim(&args[sp+1..])) { Some(v)=>v as u8, None=>{ self.write_line("  Error: valor invalido", LineColor::Error); return; }};
        unsafe { core::ptr::write_volatile(addr as *mut u8, val); }
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Escrito 0x");
        const H: &[u8] = b"0123456789ABCDEF";
        buf[pos]=H[(val>>4) as usize]; pos+=1; buf[pos]=H[(val&0xF) as usize]; pos+=1;
        append_str(&mut buf, &mut pos, b" en 0x"); append_hex64_short(&mut buf, &mut pos, addr);
        self.write_bytes(&buf[..pos], LineColor::Success);
    }

    fn cmd_cpuid(&mut self, args: &[u8]) {
        let leaf = if args.is_empty() { 0 }
                   else { match parse_u64(trim(args)) { Some(n)=>n as u32, None=>0 } };
        let (eax, ebx, ecx, edx): (u32,u32,u32,u32);
        unsafe {
            core::arch::asm!(
                "push rbx","cpuid","mov {ebx_out:e}, ebx","pop rbx",
                inout("eax") leaf => eax, ebx_out=out(reg) ebx,
                out("ecx") ecx, out("edx") edx, options(nostack, nomem)
            );
        }
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  CPUID hoja 0x");
          append_hex64_short(&mut buf,&mut pos,leaf as u64);
          self.write_bytes(&buf[..pos], LineColor::Info); }
        macro_rules! rl { ($n:expr,$v:expr)=>{{ let mut b=[0u8;80]; let mut p=0;
          append_str(&mut b,&mut p,b"    "); append_str(&mut b,&mut p,$n);
          append_str(&mut b,&mut p,b" = 0x"); append_hex64_full(&mut b,&mut p,$v as u64);
          append_str(&mut b,&mut p,b" ("); append_u32(&mut b,&mut p,$v);
          append_str(&mut b,&mut p,b")"); self.write_bytes(&b[..p],LineColor::Normal); }}}
        rl!(b"EAX",eax); rl!(b"EBX",ebx); rl!(b"ECX",ecx); rl!(b"EDX",edx);
        if leaf == 0 {
            let mut vs=[0u8;12];
            vs[0..4].copy_from_slice(&ebx.to_le_bytes());
            vs[4..8].copy_from_slice(&edx.to_le_bytes());
            vs[8..12].copy_from_slice(&ecx.to_le_bytes());
            if let Ok(s) = core::str::from_utf8(&vs) {
                let mut buf=[0u8;80]; let mut pos=0;
                append_str(&mut buf,&mut pos,b"    Fabricante: ");
                let sl=s.as_bytes(); let ll=sl.len().min(60);
                buf[pos..pos+ll].copy_from_slice(&sl[..ll]); pos+=ll;
                self.write_bytes(&buf[..pos], LineColor::Success);
            }
        }
    }

    fn cmd_pic(&mut self) {
        let (mask1,mask2):(u8,u8);
        unsafe {
            core::arch::asm!("in al, 0x21",out("al") mask1,options(nostack,nomem));
            core::arch::asm!("in al, 0xA1",out("al") mask2,options(nostack,nomem));
        }
        self.separador("ESTADO DE INTERRUPCIONES (PIC)");
        self.write_line("  IRQ  Chip  Enmascarado  Nombre", LineColor::Header);
        let nombres: &[&str] = &[
            "Temporizador PIT","Teclado","Cascada (PIC2)","COM2","COM1",
            "LPT2","Floppy","LPT1/Espuria",
            "CMOS/RTC","Libre","Libre","Libre",
            "Raton PS/2","FPU","ATA Primario","ATA Secundario",
        ];
        for irq in 0u8..16 {
            let masked = if irq<8 { mask1&(1<<irq)!=0 } else { mask2&(1<<(irq-8))!=0 };
            let chip   = if irq<8 { "PIC1" } else { "PIC2" };
            let nombre = if (irq as usize) < nombres.len() { nombres[irq as usize] } else { "?" };
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"   ");
            if irq<10 { buf[pos]=b' '; pos+=1; }
            append_u32(&mut buf,&mut pos,irq as u32);
            append_str(&mut buf,&mut pos,b"   "); append_str(&mut buf,&mut pos,chip.as_bytes());
            append_str(&mut buf,&mut pos,b"     ");
            append_str(&mut buf,&mut pos,if masked { b"  SI    " } else { b"  NO    " });
            let nl=nombre.as_bytes(); let ll=nl.len().min(30);
            buf[pos..pos+ll].copy_from_slice(&nl[..ll]); pos+=ll;
            self.write_bytes(&buf[..pos], if masked { LineColor::Normal } else { LineColor::Success });
        }
        self.write_empty();
    }

    fn cmd_gdt(&mut self) {
        let mut gdtr=[0u8;10];
        unsafe { core::arch::asm!("sgdt [{}]",in(reg) gdtr.as_mut_ptr(),options(nostack)); }
        let limit=u16::from_le_bytes([gdtr[0],gdtr[1]]);
        let base =u64::from_le_bytes([gdtr[2],gdtr[3],gdtr[4],gdtr[5],gdtr[6],gdtr[7],0,0]);
        self.separador("TABLA DE DESCRIPTORES GLOBALES (GDT)");
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  Base: 0x"); append_hex64_short(&mut buf,&mut pos,base);
          append_str(&mut buf,&mut pos,b"   Limite: "); append_u32(&mut buf,&mut pos,limit as u32);
          self.write_bytes(&buf[..pos],LineColor::Info); }
        self.write_line("  Idx  Selector  Base       Limite    Tipo", LineColor::Header);
        let count=((limit as usize+1)/8).min(8);
        for i in 0..count {
            let raw=unsafe { core::ptr::read_volatile((base+(i*8) as u64) as *const u64) };
            let bl=(raw&0xFFFF) as u32; let bm=((raw>>16)&0xFF) as u32; let bh=((raw>>56)&0xFF) as u32;
            let sb=bl|(bm<<16)|(bh<<24);
            let sl=((raw>>32)&0xFFFF) as u32|(((raw>>48)&0xF) as u32)<<16;
            let access=((raw>>40)&0xFF) as u8;
            let sys=if access&0x10!=0 { b"Cod/Dat" as &[u8] } else { b"Sistema" };
            let dpl=(access>>5)&3;
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  "); append_u32(&mut buf,&mut pos,i as u32);
            append_str(&mut buf,&mut pos,b"    0x"); append_hex64_short(&mut buf,&mut pos,(i*8) as u64);
            append_str(&mut buf,&mut pos,b"   0x"); append_hex64_short(&mut buf,&mut pos,sb as u64);
            append_str(&mut buf,&mut pos,b"   0x"); append_hex64_short(&mut buf,&mut pos,sl as u64);
            append_str(&mut buf,&mut pos,b"  "); buf[pos..pos+sys.len()].copy_from_slice(sys); pos+=sys.len();
            append_str(&mut buf,&mut pos,b" DPL"); append_u32(&mut buf,&mut pos,dpl as u32);
            self.write_bytes(&buf[..pos], if i==0 { LineColor::Normal } else { LineColor::Info });
        }
        self.write_empty();
    }

    fn cmd_memtest(&mut self, args: &[u8]) {
        let args=trim(args);
        let (addr,size)=if args.is_empty() { (0x10_0000u64,4096usize) } else {
            let sp=args.iter().position(|&b| b==b' ');
            let ap=if let Some(i)=sp { &args[..i] } else { args };
            let sp2=if let Some(i)=sp { trim(&args[i+1..]) } else { &b""[..] };
            (parse_hex(ap).unwrap_or(0x10_0000), parse_u64(sp2).unwrap_or(4096).min(65536) as usize)
        };
        self.separador("PRUEBA DE MEMORIA");
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  Direccion: 0x"); append_hex64_short(&mut buf,&mut pos,addr);
          append_str(&mut buf,&mut pos,b"   Tamano: "); append_u32(&mut buf,&mut pos,size as u32);
          append_str(&mut buf,&mut pos,b" bytes  (4 patrones)");
          self.write_bytes(&buf[..pos],LineColor::Info); }
        let patterns:&[u8]=&[0xAA,0x55,0x00,0xFF];
        let mut errors=0u32;
        for &pat in patterns {
            for i in 0..size { unsafe { core::ptr::write_volatile((addr+i as u64) as *mut u8,pat); }}
            for i in 0..size { let r=unsafe { core::ptr::read_volatile((addr+i as u64) as *const u8) }; if r!=pat { errors+=1; }}
            for i in 0..size { unsafe { core::ptr::write_volatile((addr+i as u64) as *mut u8,0); }}
        }
        if errors==0 {
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  [OK] APROBADO: "); append_u32(&mut buf,&mut pos,size as u32);
            append_str(&mut buf,&mut pos,b" bytes sin errores");
            self.write_bytes(&buf[..pos],LineColor::Success);
        } else {
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  [!!] FALLO: "); append_u32(&mut buf,&mut pos,errors);
            append_str(&mut buf,&mut pos,b" errores encontrados");
            self.write_bytes(&buf[..pos],LineColor::Error);
        }
        self.write_empty();
    }

    fn cmd_inb(&mut self, args: &[u8]) {
        let args=trim(args);
        if args.is_empty() { self.write_line("  Uso: inb <0xPUERTO>",LineColor::Warning); return; }
        let port=match parse_hex(args) { Some(p)=>p as u16, None=>{ self.write_line("  Error: puerto invalido",LineColor::Error); return; }};
        let val:u8;
        unsafe { core::arch::asm!("in al, dx",out("al") val,in("dx") port,options(nostack,nomem)); }
        let mut buf=[0u8;80]; let mut pos=0;
        append_str(&mut buf,&mut pos,b"  inb(0x"); append_hex64_short(&mut buf,&mut pos,port as u64);
        append_str(&mut buf,&mut pos,b") = 0x");
        const H:&[u8]=b"0123456789ABCDEF";
        buf[pos]=H[(val>>4) as usize]; pos+=1; buf[pos]=H[(val&0xF) as usize]; pos+=1;
        append_str(&mut buf,&mut pos,b" ("); append_u32(&mut buf,&mut pos,val as u32);
        append_str(&mut buf,&mut pos,b")");
        self.write_bytes(&buf[..pos],LineColor::Success);
    }

    fn cmd_outb(&mut self, args: &[u8]) {
        let args=trim(args);
        let sp=match args.iter().position(|&b| b==b' ') { Some(i)=>i, None=>{ self.write_line("  Uso: outb <0xPUERTO> <valor>",LineColor::Warning); return; }};
        let port=match parse_hex(&args[..sp]) { Some(p)=>p as u16, None=>{ self.write_line("  Error: puerto invalido",LineColor::Error); return; }};
        let val =match parse_u64(trim(&args[sp+1..])) { Some(v)=>v as u8, None=>{ self.write_line("  Error: valor invalido",LineColor::Error); return; }};
        unsafe { core::arch::asm!("out dx, al",in("dx") port,in("al") val,options(nostack,nomem)); }
        let mut buf=[0u8;80]; let mut pos=0;
        append_str(&mut buf,&mut pos,b"  outb(0x"); append_hex64_short(&mut buf,&mut pos,port as u64);
        append_str(&mut buf,&mut pos,b", 0x");
        const H:&[u8]=b"0123456789ABCDEF";
        buf[pos]=H[(val>>4) as usize]; pos+=1; buf[pos]=H[(val&0xF) as usize]; pos+=1;
        append_str(&mut buf,&mut pos,b") completado");
        self.write_bytes(&buf[..pos],LineColor::Success);
    }

    fn cmd_beep(&mut self, args: &[u8]) {
        let freq=if args.is_empty() { 440u32 } else {
            match parse_u64(trim(args)) { Some(f)=>(f as u32).max(20).min(20000), None=>440 }};
        let div=1193182u32/freq;
        unsafe {
            core::arch::asm!("out 0x43, al",in("al") 0xB6u8,options(nostack,nomem));
            core::arch::asm!("out 0x42, al",in("al") (div&0xFF) as u8,options(nostack,nomem));
            core::arch::asm!("out 0x42, al",in("al") ((div>>8)&0xFF) as u8,options(nostack,nomem));
            let mut p:u8; core::arch::asm!("in al, 0x61",out("al") p,options(nostack,nomem));
            p|=0x03; core::arch::asm!("out 0x61, al",in("al") p,options(nostack,nomem));
        }
        let start=crate::time::pit::ticks();
        while crate::time::pit::ticks().wrapping_sub(start)<20 {
            unsafe { core::arch::asm!("pause",options(nostack,nomem)); }
        }
        unsafe {
            let mut p:u8; core::arch::asm!("in al, 0x61",out("al") p,options(nostack,nomem));
            p&=!0x03; core::arch::asm!("out 0x61, al",in("al") p,options(nostack,nomem));
        }
        let mut buf=[0u8;80]; let mut pos=0;
        append_str(&mut buf,&mut pos,b"  Pitido a "); append_u32(&mut buf,&mut pos,freq);
        append_str(&mut buf,&mut pos,b" Hz (200ms)");
        self.write_bytes(&buf[..pos],LineColor::Success);
    }

    fn cmd_rgb(&mut self, args: &[u8]) {
        let args=trim(args);
        if args.starts_with(b"#")||args.starts_with(b"0x") {
            let hex=if args.starts_with(b"#") { &args[1..] } else { &args[2..] };
            let val=match parse_hex_raw(hex) { Some(v)=>v, None=>{ self.write_line("  Error: color hex invalido",LineColor::Error); return; }};
            let r=((val>>16)&0xFF) as u8; let g=((val>>8)&0xFF) as u8; let b=(val&0xFF) as u8;
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  RGB("); append_u32(&mut buf,&mut pos,r as u32);
            append_str(&mut buf,&mut pos,b", "); append_u32(&mut buf,&mut pos,g as u32);
            append_str(&mut buf,&mut pos,b", "); append_u32(&mut buf,&mut pos,b as u32);
            append_str(&mut buf,&mut pos,b")  =  0x"); append_hex64_short(&mut buf,&mut pos,val);
            self.write_bytes(&buf[..pos],LineColor::Success);
        } else {
            let mut nums=[0u32;3]; let mut ni=0; let mut start=0;
            for i in 0..=args.len() {
                let at_space=i==args.len()||args[i]==b' ';
                if at_space&&i>start&&ni<3 {
                    if let Some(n)=parse_u64(&args[start..i]) { nums[ni]=n as u32&0xFF; ni+=1; }
                    start=i+1;
                }
            }
            if ni<3 { self.write_line("  Uso: rgb <r> <g> <b>  o  rgb #RRGGBB",LineColor::Warning); return; }
            let val=((nums[0] as u64)<<16)|((nums[1] as u64)<<8)|nums[2] as u64;
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  RGB("); append_u32(&mut buf,&mut pos,nums[0]);
            append_str(&mut buf,&mut pos,b", "); append_u32(&mut buf,&mut pos,nums[1]);
            append_str(&mut buf,&mut pos,b", "); append_u32(&mut buf,&mut pos,nums[2]);
            append_str(&mut buf,&mut pos,b")  =  0x"); append_hex64_short(&mut buf,&mut pos,val);
            self.write_bytes(&buf[..pos],LineColor::Success);
        }
    }

    fn cmd_calc(&mut self, args: &[u8]) {
        if args.is_empty() { self.write_line("  Uso: calc 2+3*4   o   = 100/7",LineColor::Warning); return; }
        match simple_eval(args) {
            Some(r) => {
                let mut buf=[0u8;80]; let mut pos=0;
                append_str(&mut buf,&mut pos,b"  = ");
                if r<0 { buf[pos]=b'-'; pos+=1; append_u32(&mut buf,&mut pos,(-r) as u32); }
                else { append_u32(&mut buf,&mut pos,r as u32); }
                append_str(&mut buf,&mut pos,b"  (0x"); append_hex64_short(&mut buf,&mut pos,r as u64);
                append_str(&mut buf,&mut pos,b")");
                self.write_bytes(&buf[..pos],LineColor::Success);
            }
            None => self.write_line("  Error: expresion invalida",LineColor::Error),
        }
    }

    fn cmd_hex(&mut self, args: &[u8]) {
        if args.is_empty() { self.write_line("  Uso: hex <decimal>",LineColor::Warning); return; }
        if let Some(n)=parse_u64(trim(args)) {
            let mut buf=[0u8;80]; let mut pos=0;
            append_u32(&mut buf,&mut pos,(n&0xFFFF_FFFF) as u32);
            append_str(&mut buf,&mut pos,b" = 0x"); append_hex64_short(&mut buf,&mut pos,n);
            self.write_bytes(&buf[..pos],LineColor::Success);
        } else { self.write_line("  Error: numero decimal invalido",LineColor::Error); }
    }

    fn cmd_dec(&mut self, args: &[u8]) {
        if args.is_empty() { self.write_line("  Uso: dec <0xHEX>",LineColor::Warning); return; }
        if let Some(n)=parse_hex(trim(args)) {
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"0x"); append_hex64_short(&mut buf,&mut pos,n);
            append_str(&mut buf,&mut pos,b" = "); append_u32(&mut buf,&mut pos,(n&0xFFFF_FFFF) as u32);
            self.write_bytes(&buf[..pos],LineColor::Success);
        } else { self.write_line("  Error: hexadecimal invalido",LineColor::Error); }
    }

    fn cmd_bin(&mut self, args: &[u8]) {
        if args.is_empty() { self.write_line("  Uso: bin <decimal>",LineColor::Warning); return; }
        if let Some(n)=parse_u64(trim(args)) {
            let v=n&0xFFFF_FFFF;
            let mut buf=[0u8;80]; let mut pos=0;
            append_u32(&mut buf,&mut pos,v as u32); append_str(&mut buf,&mut pos,b" = 0b");
            let bits=if v==0 { 1 } else { (64-v.leading_zeros() as usize+3)/4*4 };
            for i in (0..bits).rev() {
                buf[pos]=if (v>>i)&1!=0 { b'1' } else { b'0' }; pos+=1;
                if i>0&&i%4==0 { buf[pos]=b'_'; pos+=1; }
            }
            self.write_bytes(&buf[..pos],LineColor::Success);
        } else { self.write_line("  Error: decimal invalido",LineColor::Error); }
    }

    fn cmd_neofetch(&mut self, hw: &crate::arch::hardware::HardwareInfo, pci: &crate::drivers::bus::pci::PciBus) {
        self.write_empty();
        let brand=hw.cpu.brand_str(); let brand=if brand.len()>36 { &brand[..36] } else { brand };
        let usable=hw.ram.usable_or_default();
        let logo:&[&str]=&[
            "     ____   ___  ____  _____ _____  __  __",
            "    |  _ \\ / _ \\|  _ \\|_   _|_   _| \\ \\/ /",
            "    | |_) | | | | |_) | | |   | |    \\  / ",
            "    |  __/| |_| |  _ <  | |   | |    /  \\ ",
            "    |_|    \\___/|_| \\_\\ |_|   |_|   /_/\\_\\",
            "                                            ",
        ];
        let mut il:[[u8;80];14]=[[0u8;80];14]; let mut ils:[usize;14]=[0;14]; let mut n=0;
        macro_rules! iline { ($k:literal,$v:expr) => {{
            let mut buf=[0u8;80]; let mut pos=0;
            append_str(&mut buf,&mut pos,b"  "); append_str(&mut buf,&mut pos,$k);
            append_str(&mut buf,&mut pos,b": ");
            let vb=$v.as_bytes(); let vl=vb.len().min(50);
            buf[pos..pos+vl].copy_from_slice(&vb[..vl]); pos+=vl;
            il[n]=buf; ils[n]=pos; n+=1;
        }}}
        iline!(b"SO       ","PORTIX v0.7 bare-metal");
        iline!(b"Arq      ","x86_64");
        iline!(b"Kernel   ","Rust nightly (no_std) + NASM");
        iline!(b"Video    ","VESA LFB (doble buffer @ 0x600000)");
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  CPU     : ");
          let bb=brand.as_bytes(); let bl=bb.len().min(50);
          buf[pos..pos+bl].copy_from_slice(&bb[..bl]); pos+=bl;
          il[n]=buf; ils[n]=pos; n+=1; }
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  Nucleos : ");
          append_u32(&mut buf,&mut pos,hw.cpu.physical_cores as u32);
          append_str(&mut buf,&mut pos,b"C / ");
          append_u32(&mut buf,&mut pos,hw.cpu.logical_cores as u32);
          append_str(&mut buf,&mut pos,b"T  @");
          append_mhz(&mut buf,&mut pos,hw.cpu.max_mhz);
          il[n]=buf; ils[n]=pos; n+=1; }
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  RAM     : ");
          append_mib(&mut buf,&mut pos,usable);
          il[n]=buf; ils[n]=pos; n+=1; }
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  Pantalla: ");
          append_u32(&mut buf,&mut pos,hw.display.width as u32);
          append_str(&mut buf,&mut pos,b"x");
          append_u32(&mut buf,&mut pos,hw.display.height as u32);
          append_str(&mut buf,&mut pos,b" @ ");
          append_u32(&mut buf,&mut pos,hw.display.bpp as u32);
          append_str(&mut buf,&mut pos,b"bpp");
          il[n]=buf; ils[n]=pos; n+=1; }
        { let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  PCI     : ");
          append_u32(&mut buf,&mut pos,pci.count as u32);
          append_str(&mut buf,&mut pos,b" disp.  Discos: ");
          append_u32(&mut buf,&mut pos,hw.disks.count as u32);
          il[n]=buf; ils[n]=pos; n+=1; }
        { let (h,m,s)=crate::time::pit::uptime_hms();
          let mut buf=[0u8;80]; let mut pos=0;
          append_str(&mut buf,&mut pos,b"  Uptime  : ");
          append_u32(&mut buf,&mut pos,h); append_str(&mut buf,&mut pos,b"h ");
          append_u32(&mut buf,&mut pos,m); append_str(&mut buf,&mut pos,b"m ");
          append_u32(&mut buf,&mut pos,s); append_str(&mut buf,&mut pos,b"s");
          il[n]=buf; ils[n]=pos; n+=1; }
        let rows=logo.len().max(n);
        for row in 0..rows {
            let mut combined=[0u8;TERM_COLS]; let mut cp=0;
            if row<logo.len() { let lb=logo[row].as_bytes(); let ll=lb.len().min(44); combined[cp..cp+ll].copy_from_slice(&lb[..ll]); }
            cp=46;
            if row<n { let l=ils[row].min(TERM_COLS.saturating_sub(cp)); combined[cp..cp+l].copy_from_slice(&il[row][..l]); cp+=l; }
            let col=if row<logo.len() { LineColor::Header } else { LineColor::Normal };
            if cp>0 { self.write_bytes(&combined[..cp],col); }
        }
        self.write_empty();
    }
}

// ══ Evaluador aritmético ═══════════════════════════════════════════════════════
fn simple_eval(expr: &[u8]) -> Option<i64> {
    let mut tokens=[(0i64,b'+');32]; let mut tcount=0usize;
    let mut i=0usize; let mut first=true;
    while i<expr.len() {
        while i<expr.len()&&expr[i]==b' ' { i+=1; }
        if i>=expr.len() { break; }
        let neg=if expr[i]==b'-'&&first { i+=1; true } else { false };
        let mut n:i64=0; let mut digits=0;
        while i<expr.len()&&expr[i].is_ascii_digit() { n=n*10+(expr[i]-b'0') as i64; i+=1; digits+=1; }
        if digits==0&&!neg { return None; }
        if neg { n=-n; }
        while i<expr.len()&&expr[i]==b' ' { i+=1; }
        let op=if i<expr.len() { let o=expr[i]; i+=1; o } else { b'+' };
        if tcount<32 { tokens[tcount]=(n,op); tcount+=1; }
        first=false;
    }
    if tcount==0 { return None; }
    let mut vals=[0i64;32]; let mut ops=[b'+';32]; let mut vn=0usize;
    let (mut acc,mut cur_op)=(tokens[0].0,tokens[0].1);
    for t in 1..tcount {
        let (num,next_op)=tokens[t];
        if cur_op==b'*' { acc*=num; }
        else if cur_op==b'/' { if num==0 { return None; } acc/=num; }
        else { vals[vn]=acc; ops[vn]=cur_op; vn+=1; acc=num; }
        cur_op=next_op;
    }
    vals[vn]=acc; vn+=1;
    let mut result=vals[0];
    for k in 1..vn { if ops[k-1]==b'+' { result+=vals[k]; } else if ops[k-1]==b'-' { result-=vals[k]; } }
    Some(result)
}

fn parse_u64(s: &[u8]) -> Option<u64> {
    let s=trim(s); if s.is_empty() { return None; }
    let mut n=0u64;
    for &b in s { if !b.is_ascii_digit() { return None; } n=n.wrapping_mul(10).wrapping_add((b-b'0') as u64); }
    Some(n)
}
fn parse_hex(s: &[u8]) -> Option<u64> {
    let s=trim(s);
    let s=if s.starts_with(b"0x")||s.starts_with(b"0X") { &s[2..] } else { s };
    parse_hex_raw(s)
}
fn parse_hex_raw(s: &[u8]) -> Option<u64> {
    if s.is_empty() { return None; }
    let mut n=0u64;
    for &b in s {
        let d=match b { b'0'..=b'9'=>b-b'0', b'a'..=b'f'=>b-b'a'+10, b'A'..=b'F'=>b-b'A'+10, _=>return None };
        n=n.wrapping_shl(4).wrapping_add(d as u64);
    }
    Some(n)
}
fn trim(s: &[u8]) -> &[u8] {
    let s=match s.iter().position(|&b| b!=b' ') { Some(i)=>&s[i..], None=>&[] };
    match s.iter().rposition(|&b| b!=b' ') { Some(i)=>&s[..=i], None=>s }
}

// ══ Formateadores ═════════════════════════════════════════════════════════════

fn append_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) {
    let l=s.len().min(buf.len().saturating_sub(*pos));
    buf[*pos..*pos+l].copy_from_slice(&s[..l]); *pos+=l;
}
fn append_u32(buf: &mut [u8], pos: &mut usize, mut n: u32) {
    let mut tmp=[0u8;10];
    if n==0 { append_str(buf,pos,b"0"); return; }
    let mut i=0; while n>0 { tmp[i]=b'0'+(n%10) as u8; n/=10; i+=1; }
    tmp[..i].reverse(); append_str(buf,pos,&tmp[..i]);
}
fn append_hex8_byte(buf: &mut [u8], pos: &mut usize, v: u8) {
    const H:&[u8]=b"0123456789ABCDEF";
    append_str(buf,pos,&[H[(v>>4) as usize],H[(v&0xF) as usize]]);
}
fn append_hex16(buf: &mut [u8], pos: &mut usize, v: u16) {
    const H:&[u8]=b"0123456789ABCDEF";
    append_str(buf,pos,&[H[((v>>12)&0xF) as usize],H[((v>>8)&0xF) as usize],H[((v>>4)&0xF) as usize],H[(v&0xF) as usize]]);
}
fn append_hex64_full(buf: &mut [u8], pos: &mut usize, mut v: u64) {
    const H:&[u8]=b"0123456789ABCDEF";
    let mut tmp=[0u8;16];
    for i in (0..16).rev() { tmp[i]=H[(v&0xF) as usize]; v>>=4; }
    append_str(buf,pos,&tmp);
}
fn append_hex64_short(buf: &mut [u8], pos: &mut usize, mut v: u64) {
    const H:&[u8]=b"0123456789ABCDEF";
    let mut tmp=[0u8;16];
    for i in (0..16).rev() { tmp[i]=H[(v&0xF) as usize]; v>>=4; }
    let start=tmp.iter().position(|&b| b!=b'0').unwrap_or(7).min(7);
    append_str(buf,pos,&tmp[start..]);
}
fn append_mhz(buf: &mut [u8], pos: &mut usize, mhz: u32) {
    if mhz>=1000 {
        let gi=mhz/1000; let gf=(mhz%1000)/10;
        append_u32(buf,pos,gi); append_str(buf,pos,b".");
        if gf<10 { append_str(buf,pos,b"0"); }
        append_u32(buf,pos,gf); append_str(buf,pos,b" GHz");
    } else { append_u32(buf,pos,mhz); append_str(buf,pos,b" MHz"); }
}
fn append_mib(buf: &mut [u8], pos: &mut usize, mb: u64) {
    if mb==0 { append_str(buf,pos,b"0 MB"); return; }
    if mb>=1024 {
        append_u32(buf,pos,(mb/1024) as u32); append_str(buf,pos,b".");
        append_u32(buf,pos,((mb%1024)*10/1024) as u32); append_str(buf,pos,b" GB");
    } else { append_u32(buf,pos,mb as u32); append_str(buf,pos,b" MB"); }
}