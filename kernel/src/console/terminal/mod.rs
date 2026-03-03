// console/terminal/mod.rs — PORTIX Kernel v0.7.5
// Struct Terminal + toda la lógica core.
// CAMBIOS v0.7.5:
//   - Añadido campo `cwd` / `cwd_len` para directorio de trabajo actual.
//   - El CWD persiste entre comandos dentro de la sesión.
//   - El CWD inicial es "/home/user" (coincide con mkfs).

#![allow(dead_code)]

pub mod fmt;
pub mod commands;
pub mod editor;

// ── Constantes públicas ───────────────────────────────────────────────────────

pub const TERM_COLS:   usize = 92;
pub const TERM_ROWS:   usize = 128;
pub const INPUT_MAX:   usize = 80;
pub const PROMPT:      &[u8] = b"PORTIX> ";
pub const SCROLL_STEP: usize = 3;
pub const CWD_MAX:     usize = 256;

// ── Tipos públicos ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineColor { Normal, Success, Warning, Error, Info, Prompt, Header }

#[derive(Clone, Copy)]
pub struct TermLine {
    pub buf:   [u8; TERM_COLS],
    pub len:   usize,
    pub color: LineColor,
}
impl TermLine {
    pub const fn empty() -> Self {
        TermLine { buf: [0; TERM_COLS], len: 0, color: LineColor::Normal }
    }
}

// ── Struct principal ──────────────────────────────────────────────────────────

pub struct Terminal {
    // Render / ring buffer
    pub lines:         [TermLine; TERM_ROWS],
    pub line_count:    usize,
    // Input en curso
    pub input:         [u8; INPUT_MAX],
    pub input_len:     usize,
    pub cursor_vis:    bool,
    // Scroll
    pub scroll_offset: usize,
    // Historial de comandos (ring buffer de 16)
    pub(crate) hist_cmds:  [[u8; INPUT_MAX]; 16],
    pub(crate) hist_lens:  [usize; 16],
    pub(crate) hist_count: usize,
    // Editor (hex o texto) — Some = editor activo, None = terminal normal
    pub editor: Option<editor::EditorState>,
    // Directorio de trabajo actual (CWD), persiste entre comandos
    pub cwd:     [u8; CWD_MAX],
    pub cwd_len: usize,
}

impl Terminal {
    pub const fn new() -> Self {
        // CWD inicial: "/home/user"
        let mut cwd = [0u8; CWD_MAX];
        cwd[0]  = b'/'; cwd[1]  = b'h'; cwd[2]  = b'o';
        cwd[3]  = b'm'; cwd[4]  = b'e'; cwd[5]  = b'/';
        cwd[6]  = b'u'; cwd[7]  = b's'; cwd[8]  = b'e'; cwd[9] = b'r';
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
            editor:        None,
            cwd,
            cwd_len:       10, // len("/home/user")
        }
    }

    // ══ Escritura ═════════════════════════════════════════════════════════════

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

    pub fn separador(&mut self, titulo: &str) {
        let tb = titulo.as_bytes();
        let tl = tb.len();
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        use fmt::append_str;
        append_str(&mut buf, &mut pos, b"  +-- ");
        let l = tl.min(60);
        buf[pos..pos + l].copy_from_slice(&tb[..l]); pos += l;
        append_str(&mut buf, &mut pos, b" ");
        let dashes = 58usize.saturating_sub(tl);
        for _ in 0..dashes.min(TERM_COLS.saturating_sub(pos).saturating_sub(2)) {
            buf[pos] = b'-'; pos += 1;
        }
        buf[pos] = b'+'; pos += 1;
        self.write_bytes(&buf[..pos], LineColor::Header);
    }

    // ══ CWD ══════════════════════════════════════════════════════════════════

    /// Retorna el CWD como &str (best-effort, '/' si UTF-8 falla).
    pub fn cwd_str(&self) -> &str {
        core::str::from_utf8(&self.cwd[..self.cwd_len]).unwrap_or("/")
    }

    // ══ Ring buffer ═══════════════════════════════════════════════════════════

    #[inline]
    pub fn line_at(&self, li: usize) -> &TermLine {
        &self.lines[li % TERM_ROWS]
    }

    #[inline]
    fn oldest_logical(&self) -> usize {
        if self.line_count <= TERM_ROWS { 0 } else { self.line_count - TERM_ROWS }
    }

    pub fn max_scroll(&self, max_visible: usize) -> usize {
        let available = self.line_count.saturating_sub(self.oldest_logical());
        available.saturating_sub(max_visible)
    }

    // ══ Scroll ════════════════════════════════════════════════════════════════

    pub fn scroll_up(&mut self, lines: usize, max_visible: usize) {
        let max = self.max_scroll(max_visible);
        self.scroll_offset = (self.scroll_offset + lines).min(max);
    }
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }
    pub fn scroll_to_bottom(&mut self) { self.scroll_offset = 0; }
    pub fn at_bottom(&self)  -> bool   { self.scroll_offset == 0 }

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

    // ══ Input ═════════════════════════════════════════════════════════════════

    pub fn type_char(&mut self, c: u8) {
        if self.input_len < INPUT_MAX - 1 && c >= 32 && c < 127 {
            self.input[self.input_len] = c;
            self.input_len += 1;
        }
    }
    pub fn backspace(&mut self) {
        if self.input_len > 0 { self.input_len -= 1; }
    }
    pub fn clear_input(&mut self) {
        self.input_len = 0;
        for b in &mut self.input { *b = 0; }
    }
    pub fn clear_history(&mut self) {
        for l in &mut self.lines { l.len = 0; l.buf[0] = 0; }
        self.line_count    = 0;
        self.scroll_offset = 0;
    }

    // ══ Enter ════════════════════════════════════════════════════════════════

    pub fn enter(
        &mut self,
        hw:  &crate::arch::hardware::HardwareInfo,
        pci: &crate::drivers::bus::pci::PciBus,
    ) {
        // Echo con CWD en el prompt: "user@portix:/home/user> comando"
        let mut echo = [0u8; INPUT_MAX + 60];
        let mut ep = 0;
        // Prompt estilo Linux: "root@portix:<cwd>$ "
        for b in b"root@portix:" { echo[ep] = *b; ep += 1; }
        let cl = self.cwd_len.min(40);
        echo[ep..ep + cl].copy_from_slice(&self.cwd[..cl]); ep += cl;
        for b in b"$ " { echo[ep] = *b; ep += 1; }
        let il = self.input_len.min(INPUT_MAX);
        echo[ep..ep + il].copy_from_slice(&self.input[..il]); ep += il;
        self.write_bytes(&echo[..ep], LineColor::Prompt);

        // Historial
        if self.input_len > 0 {
            let slot = self.hist_count % 16;
            self.hist_cmds[slot][..self.input_len].copy_from_slice(&self.input[..self.input_len]);
            self.hist_lens[slot] = self.input_len;
            self.hist_count += 1;
        }

        // Parsear cmd y args
        let mut cmd_buf  = [0u8; INPUT_MAX];
        let mut args_buf = [0u8; INPUT_MAX];
        let cmd_len; let args_len;
        {
            let raw     = &self.input[..self.input_len];
            let start   = raw.iter().position(|&b| b != b' ').unwrap_or(0);
            let trimmed = &raw[start..];
            let end     = trimmed.iter().rposition(|&b| b != b' ').map(|i| i + 1).unwrap_or(0);
            let cmd_bytes = &trimmed[..end];
            let split     = cmd_bytes.iter().position(|&b| b == b' ');
            let (cmd_tok, args) = if let Some(sp) = split {
                (&cmd_bytes[..sp], &cmd_bytes[sp + 1..])
            } else {
                (cmd_bytes, &b""[..])
            };
            cmd_len  = cmd_tok.len().min(INPUT_MAX);
            args_len = args.len().min(INPUT_MAX);
            cmd_buf [..cmd_len ].copy_from_slice(&cmd_tok[..cmd_len]);
            args_buf[..args_len].copy_from_slice(&args[..args_len]);
        }

        if cmd_len == 0 { self.clear_input(); return; }

        commands::dispatch(self, &cmd_buf[..cmd_len], &args_buf[..args_len], hw, pci);
        self.clear_input();
    }
}