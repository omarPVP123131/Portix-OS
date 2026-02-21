// kernel/src/terminal.rs — PORTIX Interactive Terminal v2
// New commands: poweroff, reboot, pci, uptime, serial
#![allow(dead_code)]

pub const TERM_COLS:  usize = 92;
pub const TERM_ROWS:  usize = 40;
pub const INPUT_MAX:  usize = 80;
pub const PROMPT:     &[u8] = b"PORTIX> ";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineColor {
    Normal, Success, Warning, Error, Info, Prompt, Header,
}

#[derive(Clone, Copy)]
pub struct TermLine {
    pub buf:   [u8; TERM_COLS],
    pub len:   usize,
    pub color: LineColor,
}
impl TermLine {
    const fn empty() -> Self { TermLine { buf: [0; TERM_COLS], len: 0, color: LineColor::Normal } }
}

pub struct Terminal {
    pub lines:       [TermLine; TERM_ROWS],
    pub line_count:  usize,
    pub input:       [u8; INPUT_MAX],
    pub input_len:   usize,
    pub cursor_vis:  bool,
}

impl Terminal {
    pub const fn new() -> Self {
        Terminal {
            lines:      [TermLine::empty(); TERM_ROWS],
            line_count: 0,
            input:      [0u8; INPUT_MAX],
            input_len:  0,
            cursor_vis: true,
        }
    }

    // ── Write helpers ─────────────────────────────────────────────────────────
    pub fn write_line(&mut self, s: &str, color: LineColor) { self.write_bytes(s.as_bytes(), color); }

    pub fn write_bytes(&mut self, s: &[u8], color: LineColor) {
        let mut start = 0;
        loop {
            let end = (start + TERM_COLS).min(s.len());
            let chunk = &s[start..end];
            let row = self.line_count % TERM_ROWS;
            let len = chunk.len();
            self.lines[row].buf[..len].copy_from_slice(chunk);
            for b in &mut self.lines[row].buf[len..] { *b = 0; }
            self.lines[row].len   = len;
            self.lines[row].color = color;
            self.line_count += 1;
            start = end;
            if start >= s.len() { break; }
        }
    }

    pub fn write_empty(&mut self) { self.write_bytes(b"", LineColor::Normal); }

    // ── Input ─────────────────────────────────────────────────────────────────
    pub fn type_char(&mut self, c: u8) {
        if self.input_len < INPUT_MAX - 1 && c >= 32 && c < 127 {
            self.input[self.input_len] = c;
            self.input_len += 1;
        }
    }
    pub fn backspace(&mut self) { if self.input_len > 0 { self.input_len -= 1; } }
    pub fn clear_input(&mut self) { self.input_len = 0; for b in &mut self.input { *b = 0; } }
    pub fn clear_history(&mut self) {
        for l in &mut self.lines { l.len = 0; l.buf[0] = 0; }
        self.line_count = 0;
    }
    pub fn visible_range(&self, max_visible: usize) -> (usize, usize) {
        let total = self.line_count;
        if total <= max_visible { (0, total) } else { (total - max_visible, max_visible) }
    }

    // ── Enter ─────────────────────────────────────────────────────────────────
    pub fn enter(&mut self, hw: &crate::hardware::HardwareInfo, pci: &crate::pci::PciBus) {
        let mut echo = [0u8; INPUT_MAX + 10];
        let plen = PROMPT.len();
        echo[..plen].copy_from_slice(PROMPT);
        echo[plen..plen + self.input_len].copy_from_slice(&self.input[..self.input_len]);
        self.write_bytes(&echo[..plen + self.input_len], LineColor::Prompt);

        let mut cmd_buf  = [0u8; INPUT_MAX];
        let mut args_buf = [0u8; INPUT_MAX];
        let cmd_len;
        let args_len;
        {
            let raw     = &self.input[..self.input_len];
            let start   = raw.iter().position(|&b| b != b' ').unwrap_or(0);
            let trimmed = &raw[start..];
            let end     = trimmed.iter().rposition(|&b| b != b' ').map(|i| i + 1).unwrap_or(0);
            let cmd_bytes = &trimmed[..end];
            let split = cmd_bytes.iter().position(|&b| b == b' ');
            let (cmd_tok, args) = if let Some(sp) = split {
                (&cmd_bytes[..sp], &cmd_bytes[sp + 1..])
            } else {
                (cmd_bytes, &b""[..])
            };
            cmd_len  = cmd_tok.len().min(INPUT_MAX);
            args_len = args.len().min(INPUT_MAX);
            cmd_buf [..cmd_len ].copy_from_slice(&cmd_tok[..cmd_len ]);
            args_buf[..args_len].copy_from_slice(&args   [..args_len]);
        }

        if cmd_len == 0 { self.clear_input(); return; }
        self.dispatch(&cmd_buf[..cmd_len], &args_buf[..args_len], hw, pci);
        self.clear_input();
    }

    fn dispatch(&mut self, cmd: &[u8], args: &[u8],
                hw: &crate::hardware::HardwareInfo, pci: &crate::pci::PciBus) {
        match cmd {
            b"help" | b"?" => self.cmd_help(),
            b"clear" | b"cls" => self.clear_history(),
            b"info"    => self.cmd_info(hw),
            b"cpu"     => self.cmd_cpu(hw),
            b"mem" | b"memory"  => self.cmd_mem(hw),
            b"disks" | b"storage" => self.cmd_disks(hw),
            b"pci"     => self.cmd_pci(pci),
            b"ver" | b"version" => self.cmd_ver(),
            b"uptime"  => self.cmd_uptime(),
            b"echo"    => self.write_bytes(args, LineColor::Normal),
            b"uname"   => self.write_line("PORTIX 0.6 x86_64 bare-metal", LineColor::Normal),
            b"poweroff" | b"shutdown" => { self.write_line("Powering off...", LineColor::Warning); crate::acpi::poweroff(); }
            b"reboot"  => { self.write_line("Rebooting...", LineColor::Warning); crate::acpi::reboot(); }
            _ => {
                let mut buf = [0u8; 64];
                let mut pos = 0;
                for b in b"Command not found: " { buf[pos] = *b; pos += 1; }
                let l = cmd.len().min(40);
                buf[pos..pos+l].copy_from_slice(&cmd[..l]);
                self.write_bytes(&buf[..pos+l], LineColor::Error);
            }
        }
    }

    // ── Commands ──────────────────────────────────────────────────────────────
    fn cmd_help(&mut self) {
        self.write_line("┌─────────────────────────────────────────────┐", LineColor::Header);
        self.write_line("│          PORTIX Command Reference            │", LineColor::Header);
        self.write_line("└─────────────────────────────────────────────┘", LineColor::Header);
        self.write_empty();
        self.write_line("  help / ?         This help screen",            LineColor::Info);
        self.write_line("  info             Full hardware summary",        LineColor::Info);
        self.write_line("  cpu              CPU details & features",       LineColor::Info);
        self.write_line("  mem / memory     Memory map (E820)",           LineColor::Info);
        self.write_line("  disks / storage  ATA storage devices",         LineColor::Info);
        self.write_line("  pci              PCI bus enumeration",          LineColor::Info);
        self.write_line("  uptime           System uptime",                LineColor::Info);
        self.write_line("  ver / version    Kernel version",               LineColor::Info);
        self.write_line("  uname            OS name string",               LineColor::Info);
        self.write_line("  echo <text>      Print text",                   LineColor::Info);
        self.write_line("  clear / cls      Clear terminal",               LineColor::Info);
        self.write_line("  reboot           Restart the system",           LineColor::Warning);
        self.write_line("  poweroff         Shut down the system",         LineColor::Warning);
        self.write_empty();
        self.write_line("  F1=System  F2=Terminal  F3=Devices  TAB=cycle", LineColor::Normal);
    }

    fn cmd_ver(&mut self) {
        self.write_line("PORTIX Kernel v0.6 — x86_64 bare-metal", LineColor::Success);
        self.write_line("Build: 2026 / Rust nightly + NASM",        LineColor::Normal);
        self.write_line("Features: PIT · PS/2 KBD+Mouse · ATA · VESA · PCI · ACPI", LineColor::Info);
    }

    fn cmd_uptime(&mut self) {
        let (h, m, s) = crate::pit::uptime_hms();
        let t = crate::pit::ticks();
        let mut buf = [0u8; TERM_COLS];
        let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Uptime: ");
        append_u32(&mut buf, &mut pos, h);
        append_str(&mut buf, &mut pos, b"h ");
        append_u32(&mut buf, &mut pos, m);
        append_str(&mut buf, &mut pos, b"m ");
        append_u32(&mut buf, &mut pos, s);
        append_str(&mut buf, &mut pos, b"s  (");
        append_u32(&mut buf, &mut pos, (t & 0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b" ticks @ 100 Hz)");
        self.write_bytes(&buf[..pos], LineColor::Success);
    }

    fn cmd_info(&mut self, hw: &crate::hardware::HardwareInfo) {
        self.write_line("━━━ System Information ━━━━━━━━━━━━━━━━━━━━━━━", LineColor::Header);
        self.cmd_cpu(hw);
        self.write_empty();
        self.cmd_mem(hw);
        self.write_empty();
        self.cmd_disks(hw);
    }

    fn cmd_cpu(&mut self, hw: &crate::hardware::HardwareInfo) {
        self.write_line("━━━ CPU ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", LineColor::Header);
        {
            let mut lb = [0u8; TERM_COLS];
            let bl = b"  Model  : ";
            lb[..bl.len()].copy_from_slice(bl);
            let n = hw.cpu.brand_str().as_bytes();
            let nl = n.len().min(TERM_COLS - bl.len());
            lb[bl.len()..bl.len()+nl].copy_from_slice(&n[..nl]);
            self.write_bytes(&lb[..bl.len()+nl], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Vendor : ");
            let v = hw.cpu.vendor_str().as_bytes();
            let l = v.len().min(20);
            buf[pos..pos+l].copy_from_slice(&v[..l]); pos += l;
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Cores  : ");
            append_u32(&mut buf, &mut pos, hw.cpu.physical_cores as u32);
            append_str(&mut buf, &mut pos, b" physical / ");
            append_u32(&mut buf, &mut pos, hw.cpu.logical_cores as u32);
            append_str(&mut buf, &mut pos, b" logical");
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        if hw.cpu.max_mhz > 0 {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Freq   : max ");
            append_mhz(&mut buf, &mut pos, hw.cpu.max_mhz);
            if hw.cpu.base_mhz > 0 && hw.cpu.base_mhz != hw.cpu.max_mhz {
                append_str(&mut buf, &mut pos, b"  base ");
                append_mhz(&mut buf, &mut pos, hw.cpu.base_mhz);
            }
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  ISA    :");
            if hw.cpu.has_sse2 { append_str(&mut buf, &mut pos, b" SSE2"); }
            if hw.cpu.has_sse4 { append_str(&mut buf, &mut pos, b" SSE4"); }
            if hw.cpu.has_avx  { append_str(&mut buf, &mut pos, b" AVX");  }
            if hw.cpu.has_avx2 { append_str(&mut buf, &mut pos, b" AVX2"); }
            if hw.cpu.has_aes  { append_str(&mut buf, &mut pos, b" AES-NI"); }
            self.write_bytes(&buf[..pos], LineColor::Success);
        }
    }

    fn cmd_mem(&mut self, hw: &crate::hardware::HardwareInfo) {
        self.write_line("━━━ Memory (E820) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━", LineColor::Header);
        {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Usable RAM : ");
            append_mib(&mut buf, &mut pos, hw.ram.usable_or_default());
            append_str(&mut buf, &mut pos, b"   Entries: ");
            append_u32(&mut buf, &mut pos, hw.ram.entry_count as u32);
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
        self.write_empty();
        unsafe {
            let count = hw.ram.entry_count.min(16) as usize;
            for i in 0..count {
                let p    = (0x9102usize + i * 20) as *const u8;
                let base = core::ptr::read_unaligned(p as *const u64);
                let len  = core::ptr::read_unaligned(p.add(8) as *const u64);
                let kind = core::ptr::read_unaligned(p.add(16) as *const u32);
                let ts: &[u8] = match kind { 1=>b"Usable", 2=>b"Reserved", 3=>b"ACPI Reclm", 4=>b"ACPI NVS", 5=>b"Bad RAM", _=>b"Unknown" };
                let mut eb = [0u8; TERM_COLS]; let mut ep = 0;
                append_str(&mut eb, &mut ep, b"  ["); append_u32(&mut eb, &mut ep, i as u32);
                append_str(&mut eb, &mut ep, b"] 0x"); append_hex64(&mut eb, &mut ep, base);
                append_str(&mut eb, &mut ep, b"  +"); append_mib(&mut eb, &mut ep, len / (1024*1024));
                append_str(&mut eb, &mut ep, b"  ");
                eb[ep..ep+ts.len()].copy_from_slice(ts); ep += ts.len();
                self.write_bytes(&eb[..ep], if kind == 1 { LineColor::Success } else { LineColor::Normal });
            }
        }
    }

    fn cmd_disks(&mut self, hw: &crate::hardware::HardwareInfo) {
        self.write_line("━━━ Storage ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", LineColor::Header);
        if hw.disks.count == 0 {
            self.write_line("  No ATA drives detected", LineColor::Warning);
            return;
        }
        for i in 0..hw.disks.count {
            let d = &hw.disks.drives[i];
            let bus = if d.bus == 0 { b"ATA0" as &[u8] } else { b"ATA1" };
            let drv = if d.drive == 0 { b"M" as &[u8] } else { b"S" };
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [");
            buf[pos..pos+bus.len()].copy_from_slice(bus); pos += bus.len();
            append_str(&mut buf, &mut pos, b"-");
            buf[pos..pos+drv.len()].copy_from_slice(drv); pos += drv.len();
            append_str(&mut buf, &mut pos, if d.is_atapi { b"] OPT  " } else { b"] HDD  " });
            let m = d.model_str().as_bytes(); let ml = m.len().min(28);
            buf[pos..pos+ml].copy_from_slice(&m[..ml]); pos += ml;
            if !d.is_atapi {
                append_str(&mut buf, &mut pos, b"  ");
                append_mib(&mut buf, &mut pos, d.size_mb);
                if d.lba48 { append_str(&mut buf, &mut pos, b" [LBA48]"); }
            }
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
    }

    fn cmd_pci(&mut self, pci: &crate::pci::PciBus) {
        self.write_line("━━━ PCI Bus ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", LineColor::Header);
        if pci.count == 0 {
            self.write_line("  No PCI devices found", LineColor::Warning);
            return;
        }
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Found "); append_u32(&mut buf, &mut pos, pci.count as u32);
        append_str(&mut buf, &mut pos, b" devices:");
        self.write_bytes(&buf[..pos], LineColor::Normal);
        self.write_empty();

        for i in 0..pci.count.min(20) {
            let d = &pci.devices[i];
            let mut lb = [0u8; TERM_COLS]; let mut lp = 0;
            append_str(&mut lb, &mut lp, b"  [");
            append_u32(&mut lb, &mut lp, d.bus as u32); append_str(&mut lb, &mut lp, b":");
            append_hex8(&mut lb, &mut lp, d.device); append_str(&mut lb, &mut lp, b".");
            append_u32(&mut lb, &mut lp, d.function as u32); append_str(&mut lb, &mut lp, b"] ");
            // Vendor:Device IDs
            append_hex16(&mut lb, &mut lp, d.vendor_id); append_str(&mut lb, &mut lp, b":");
            append_hex16(&mut lb, &mut lp, d.device_id); append_str(&mut lb, &mut lp, b"  ");
            // Vendor name
            let vn = d.vendor_name().as_bytes();
            lb[lp..lp+vn.len()].copy_from_slice(vn); lp += vn.len();
            append_str(&mut lb, &mut lp, b" ");
            // Class
            let cn = d.class_name().as_bytes(); let cl = cn.len().min(20);
            lb[lp..lp+cl].copy_from_slice(&cn[..cl]); lp += cl;
            self.write_bytes(&lb[..lp], LineColor::Info);
        }
        if pci.count > 20 {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  ... and ");
            append_u32(&mut buf, &mut pos, (pci.count - 20) as u32);
            append_str(&mut buf, &mut pos, b" more");
            self.write_bytes(&buf[..pos], LineColor::Normal);
        }
    }
}

// ── Formatters ────────────────────────────────────────────────────────────────
fn append_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) {
    let l = s.len().min(buf.len().saturating_sub(*pos));
    buf[*pos..*pos+l].copy_from_slice(&s[..l]);
    *pos += l;
}
fn append_u32(buf: &mut [u8], pos: &mut usize, mut n: u32) {
    let mut tmp = [0u8; 10]; if n == 0 { tmp[0]=b'0'; append_str(buf,pos,&tmp[..1]); return; }
    let mut i=0; while n>0 { tmp[i]=b'0'+(n%10) as u8; n/=10; i+=1; } tmp[..i].reverse();
    append_str(buf,pos,&tmp[..i]);
}
fn append_hex8(buf: &mut [u8], pos: &mut usize, v: u8) {
    const H: &[u8]=b"0123456789ABCDEF";
    let tmp=[H[(v>>4) as usize], H[(v&0xF) as usize]];
    append_str(buf,pos,&tmp);
}
fn append_hex16(buf: &mut [u8], pos: &mut usize, v: u16) {
    const H: &[u8]=b"0123456789ABCDEF";
    let tmp=[H[((v>>12)&0xF) as usize], H[((v>>8)&0xF) as usize],
             H[((v>>4)&0xF) as usize],  H[(v&0xF) as usize]];
    append_str(buf,pos,&tmp);
}
fn append_hex64(buf: &mut [u8], pos: &mut usize, mut v: u64) {
    const H: &[u8]=b"0123456789ABCDEF";
    let mut tmp=[0u8;16];
    for i in (0..16).rev() { tmp[i]=H[(v&0xF) as usize]; v>>=4; }
    let start = tmp.iter().position(|&b| b!=b'0').unwrap_or(7).min(7);
    append_str(buf,pos,&tmp[start..]);
}
fn append_mhz(buf: &mut [u8], pos: &mut usize, mhz: u32) {
    if mhz>=1000 {
        let gi=mhz/1000; let gf=(mhz%1000)/10;
        append_u32(buf,pos,gi); append_str(buf,pos,b".");
        if gf<10 { append_str(buf,pos,b"0"); }
        append_u32(buf,pos,gf); append_str(buf,pos,b" GHz");
    } else {
        append_u32(buf,pos,mhz); append_str(buf,pos,b" MHz");
    }
}
fn append_mib(buf: &mut [u8], pos: &mut usize, mb: u64) {
    if mb==0 { append_str(buf,pos,b"0 MB"); return; }
    if mb>=1024 {
        append_u32(buf,pos,(mb/1024) as u32); append_str(buf,pos,b".");
        append_u32(buf,pos,((mb%1024)*10/1024) as u32); append_str(buf,pos,b" GB");
    } else {
        append_u32(buf,pos,mb as u32); append_str(buf,pos,b" MB");
    }
}