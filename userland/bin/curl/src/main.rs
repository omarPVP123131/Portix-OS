#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::{println, print, UdpSocket};

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in s.split('.') {
        if count >= 4 { return None; }
        let mut val: u16 = 0;
        if part.is_empty() { return None; }
        for &b in part.as_bytes() {
            if !b.is_ascii_digit() { return None; }
            val = val.checked_mul(10)?.checked_add((b - b'0') as u16)?;
        }
        if val > 255 { return None; }
        octets[count] = val as u8;
        count += 1;
    }
    if count == 4 { Some(octets) } else { None }
}

/// Consulta DNS (RFC 1035) de tipo `qtype` (1 = A, 28 = AAAA) para `host`.
fn build_dns_query(host: &str, qtype: u16) -> Vec<u8> {
    let mut query = Vec::with_capacity(64);
    query.push(0x13); query.push(0x37); // ID = 0x1337
    query.push(0x01); query.push(0x00); // Standard query, RD=1
    query.push(0x00); query.push(0x01); // QDCOUNT = 1
    query.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    for part in host.split('.') {
        if !part.is_empty() {
            let len = part.len().min(63) as u8;
            query.push(len);
            for &b in part.as_bytes() { query.push(b); }
        }
    }
    query.push(0x00);
    query.extend_from_slice(&qtype.to_be_bytes());
    query.push(0x00); query.push(0x01); // Class IN
    query
}

/// Extrae el primer RDATA de tipo `qtype` y longitud `rdlen` de una respuesta DNS real.
fn parse_dns_answer(rx: &[u8], qtype: u16, rdlen_esperado: usize) -> Option<Vec<u8>> {
    let n = rx.len();
    if n < 12 || rx[0] != 0x13 || rx[1] != 0x37 { return None; }
    if (rx[3] & 0x0F) != 0 { return None; } // RCODE != 0 (NXDOMAIN, SERVFAIL...)
    let qd = ((rx[4] as usize) << 8) | rx[5] as usize;
    let an = ((rx[6] as usize) << 8) | rx[7] as usize;
    let mut off = 12usize;
    for _ in 0..qd {
        while off < n && rx[off] != 0 {
            if rx[off] & 0xC0 == 0xC0 { off += 1; break; }
            off += 1 + rx[off] as usize;
        }
        off += 1 + 4;
    }
    for _ in 0..an {
        if off >= n { break; }
        if rx[off] & 0xC0 == 0xC0 { off += 2; } else {
            while off < n && rx[off] != 0 { off += 1 + rx[off] as usize; }
            off += 1;
        }
        if off + 10 > n { break; }
        let rtype = ((rx[off] as u16) << 8) | rx[off + 1] as u16;
        let rdlen = ((rx[off + 8] as usize) << 8) | rx[off + 9] as usize;
        off += 10;
        if rtype == qtype && rdlen == rdlen_esperado && off + rdlen <= n {
            return Some(rx[off..off + rdlen].to_vec());
        }
        off += rdlen;
    }
    None
}

fn resolve_dns(sock: &UdpSocket, host: &str, dns_server: [u8; 4]) -> Option<[u8; 4]> {
    let query = build_dns_query(host, 1);
    if sock.send_to(&query, dns_server, 53).is_err() { return None; }
    // [FRENTE B] Respuesta DNS REAL: se espera (recvfrom bloqueante, 5 s) y se
    // extrae el primer registro A; si no hay, None.
    let mut rx = [0u8; 512];
    let (n, _ip, _port) = match sock.recv_from(&mut rx) { Ok(v) => v, Err(_) => return None };
    let r = parse_dns_answer(&rx[..n], 1, 4)?;
    Some([r[0], r[1], r[2], r[3]])
}

/// [IPv6] Registro AAAA pedido por UDP/IPv6 al resolutor `dns6` (p. ej. fec0::3).
fn resolve_dns_aaaa(sock: &UdpSocket, host: &str, dns6: [u8; 16]) -> Option<[u8; 16]> {
    let query = build_dns_query(host, 28);
    if sock.send_to6(&query, dns6, 53).is_err() { return None; }
    let mut rx = [0u8; 512];
    let (n, src6, _port) = match sock.recv_from6(&mut rx) {
        Ok(v) => v,
        Err(e) => { println!("[CURL] recvfrom UDP/IPv6 fallo (err={}): {}", e, if e == -113 { "destino inalcanzable (ICMPv6)" } else if e == -111 { "puerto inalcanzable (ICMPv6)" } else { "sin respuesta" }); return None; }
    };
    println!("[CURL] respuesta DNS de [{}] ({} bytes por UDP/IPv6)", portix_rt::net::fmt_ipv6(&src6), n);
    let r = parse_dns_answer(&rx[..n], 28, 16)?;
    let mut a = [0u8; 16]; a.copy_from_slice(&r); Some(a)
}

/// ¿Cabeceras recibidas y cuerpo completo segun Content-Length?
fn respuesta_completa(r: &[u8]) -> bool {
    let sep = match r.windows(4).position(|w| w == b"\r\n\r\n") { Some(p) => p, None => return false };
    let cab = core::str::from_utf8(&r[..sep]).unwrap_or("");
    let cl = cab.lines().find_map(|l| { let (k, v) = l.split_once(':')?; if k.eq_ignore_ascii_case("content-length") { v.trim().parse::<usize>().ok() } else { None } });
    match cl { Some(n) => r.len() >= sep + 4 + n, None => false }
}

/// `--udp-eco-local=PUERTO`: un datagrama a 127.0.0.1:PUERTO y otro a [::1]:PUERTO desde el
/// mismo socket ligado a PUERTO; ambos deben volver por el bucle local del nucleo con su origen real.
fn udp_eco_local(puerto: u16) -> i32 {
    use portix_rt::net::UdpSocket;
    let s = match UdpSocket::bind([0, 0, 0, 0], puerto) { Ok(s) => s, Err(e) => { println!("[CURL-UDP-FAIL] bind: {}", e); return 1; } };
    let mut buf = [0u8; 64];
    if let Err(e) = s.send_to(b"PORTIX-UDP-LOOP-V4", [127, 0, 0, 1], puerto) { println!("[CURL-UDP-FAIL] sendto 127.0.0.1: {}", e); return 1; }
    match s.recv_from(&mut buf) {
        Ok((n, ip, p)) => println!("[CURL-UDP-LOOP] recibido '{}' de {}.{}.{}.{}:{}", core::str::from_utf8(&buf[..n]).unwrap_or("?"), ip[0], ip[1], ip[2], ip[3], p),
        Err(e) => { println!("[CURL-UDP-FAIL] recvfrom v4: {}", e); return 1; }
    }
    let mut v6 = [0u8; 16]; v6[15] = 1;
    if let Err(e) = s.send_to6(b"PORTIX-UDP-LOOP-V6", v6, puerto) { println!("[CURL-UDP-FAIL] sendto ::1: {}", e); return 1; }
    match s.recv_from6(&mut buf) {
        Ok((n, ip, p)) => println!("[CURL-UDP-LOOP] recibido '{}' de [{}]:{}", core::str::from_utf8(&buf[..n]).unwrap_or("?"), portix_rt::net::fmt_ipv6(&ip), p),
        Err(e) => { println!("[CURL-UDP-FAIL] recvfrom v6: {}", e); return 1; }
    }
    0
}

static HIJO_HTTPD: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32 {
    let rc = curl_main(argc, argv, envp);
    let pid = HIJO_HTTPD.load(core::sync::atomic::Ordering::Relaxed);
    if pid > 0 {
        let mut status: i32 = 0;
        let w = portix_rt::syscall::sys_waitpid(pid as i32, &mut status as *mut i32, 0);
        println!("[CURL] servidor local (PID {}) terminado: waitpid={} estado={}", pid, w, status);
    }
    rc
}

fn curl_main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut raw_url = "http://10.0.2.2/";
    let mut out_file: Option<String> = None;
    // [TLS-RESUMED] `--reanudar`: tras la descarga, segunda conexion con el
    // ticket PSK recibido. `--tls-eco`: prueba de KeyUpdate contra un servidor
    // TLS 1.3 de eco (sin HTTP).
    let mut reanudar = false;
    let mut tls_eco = false;
    // mTLS: `--cert-cliente` presenta /etc/ssl/certs/client.crt + /etc/ssl/private/client.key
    let mut cert_cliente = false;
    let mut servidor_local_ident: &str = "p256";
    // `--cert-cliente=rsa|ed25519` elige otra identidad (/etc/ssl/certs/client-rsa.crt, client-ed25519.crt)
    let mut cert_tipo: &str = "p256";
    // `--pausa-ms N`: tras conectar, espera N ms antes de enviar (permite al
    // banco de pruebas retener la red y observar la retransmision TCP real).
    let mut pausa_ms: u64 = 0;
    // `-6`: resolver por AAAA (UDP/IPv6 a fec0::3) y conectar por TCP/IPv6
    let mut solo_v6 = false;
    // `--con-servidor-local=PUERTO`: lanza (fork+execve) `/bin/httpd --https PUERTO 1 20`
    // como hijo y, tras 1,5 s, hace la peticion de la URL contra el; al final waitpid.
    let mut servidor_local: u16 = 0;
    let mut lector_lento = false;

    let mut i = 1;
    while i < argc as usize {
        if !argv.is_null() {
            unsafe {
                let arg_ptr = *argv.add(i);
                if !arg_ptr.is_null() {
                    let mut len = 0;
                    while *arg_ptr.add(len) != 0 { len += 1; }
                    if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(arg_ptr, len)) {
                        let trimmed = s.trim();
                        if trimmed == "-o" || trimmed == "--output" {
                            i += 1;
                            if i < argc as usize {
                                let o_ptr = *argv.add(i);
                                if !o_ptr.is_null() {
                                    let mut olen = 0;
                                    while *o_ptr.add(olen) != 0 { olen += 1; }
                                    if let Ok(os) = core::str::from_utf8(core::slice::from_raw_parts(o_ptr, olen)) {
                                        out_file = Some(String::from(os.trim()));
                                    }
                                }
                            }
                        } else if trimmed == "--reanudar" { reanudar = true;
                        } else if trimmed == "--tls-eco" { tls_eco = true;
                        } else if trimmed == "--cert-cliente" { cert_cliente = true;
                        } else if let Some(t) = trimmed.strip_prefix("--cert-cliente=") { cert_cliente = true; cert_tipo = if t == "rsa" { "rsa" } else if t == "ed25519" { "ed25519" } else { "p256" };
                        } else if trimmed == "-6" { solo_v6 = true;
                        } else if let Some(v) = trimmed.strip_prefix("--con-servidor-local=") {
                            // PUERTO[:p256|rsa|ed25519] — identidad TLS del servidor local
                            let (pv, iv) = match v.find(':') { Some(i) => (&v[..i], &v[i + 1..]), None => (v, "p256") };
                            servidor_local = pv.parse().unwrap_or(0); servidor_local_ident = if iv == "rsa" { "rsa" } else if iv == "ed25519" { "ed25519" } else { "p256" };
                        } else if trimmed == "--lector-lento" { lector_lento = true;
                        } else if let Some(v) = trimmed.strip_prefix("--udp-eco-local=") { return udp_eco_local(v.parse().unwrap_or(0));
                        } else if let Some(v) = trimmed.strip_prefix("--pausa-ms=") { pausa_ms = v.parse().unwrap_or(0);
                        } else if !trimmed.starts_with('-') {
                            raw_url = trimmed;
                        }
                    }
                }
            }
        }
        i += 1;
    }

    let is_https = raw_url.starts_with("https://") || tls_eco;
    let default_port = if is_https { 443 } else { 80 };

    // Strip http:// or https://
    let without_scheme = if let Some(stripped) = raw_url.strip_prefix("http://") {
        stripped
    } else if let Some(stripped) = raw_url.strip_prefix("https://") {
        stripped
    } else {
        raw_url
    };

    let (host_port, path) = if let Some(idx) = without_scheme.find('/') {
        (&without_scheme[..idx], &without_scheme[idx..])
    } else {
        (without_scheme, "/")
    };

    // [IPv6] literal entre corchetes: http://[fec0::2]:8000/ (RFC 3986 §3.2.2)
    let (host, port) = if host_port.starts_with('[') {
        match host_port.find(']') {
            Some(end) => {
                let h = &host_port[1..end];
                let rest = &host_port[end + 1..];
                let p = rest.strip_prefix(':').and_then(|x| x.parse::<u16>().ok()).unwrap_or(default_port);
                (h, p)
            }
            None => { println!("curl: URL IPv6 mal formada (falta ']')"); return 1; }
        }
    } else if let Some(colon) = host_port.find(':') {
        let h = &host_port[..colon];
        let p = host_port[colon + 1..].parse::<u16>().unwrap_or(default_port);
        (h, p)
    } else {
        (host_port, default_port)
    };

    if servidor_local != 0 {
        use portix_rt::syscall::{sys_fork, sys_execve, sys_exit};
        let puerto = alloc::format!("{}\0", servidor_local);
        let ident = alloc::format!("{}\0", servidor_local_ident);
        let pid = sys_fork();
        if pid == 0 {
            let argv: [*const u8; 8] = [b"/bin/httpd\0".as_ptr(), b"--https\0".as_ptr(), puerto.as_ptr(), b"1\0".as_ptr(), b"20\0".as_ptr(), b"-\0".as_ptr(), ident.as_ptr(), core::ptr::null()];
            let envp: [*const u8; 1] = [core::ptr::null()];
            sys_execve(b"/bin/httpd\0".as_ptr(), argv.as_ptr() as *const u8, envp.as_ptr() as *const u8);
            sys_exit(127);
        }
        HIJO_HTTPD.store(pid, core::sync::atomic::Ordering::Relaxed);
        println!("[CURL] servidor local lanzado: /bin/httpd --https {} 1 20 - {} (PID {}); esperando 1500 ms", servidor_local, servidor_local_ident, pid);
        let ts = portix_rt::syscall::Timespec { tv_sec: 1, tv_nsec: 500_000_000 };
        let _ = portix_rt::syscall::sys_nanosleep(&ts as *const _, core::ptr::null_mut());
    }
    println!("[CURL] Iniciando cliente HTTP/HTTPS de Ring 3 en Rust...");
    if host.contains(':') { println!("[CURL] Destino: [{}]:{} (ruta '{}')", host, port, path); } else { println!("[CURL] Destino: {}:{} (ruta '{}')", host, port, path); }

    let sock = match UdpSocket::bind([0, 0, 0, 0], 0) {
        Ok(s) => s,
        Err(e) => {
            println!("curl: fallo al crear socket (err={})", e);
            return 1;
        }
    };

    let mut dest_ip6 = portix_rt::net::parse_ipv6(host);
    if solo_v6 && dest_ip6.is_none() && parse_ipv4(host).is_none() {
        println!("[CURL] Resolviendo AAAA para host '{}' via UDP/IPv6 [fec0::3]:53...", host);
        let dns6 = portix_rt::net::parse_ipv6("fec0::3").unwrap_or([0; 16]);
        match resolve_dns_aaaa(&sock, host, dns6) {
            Some(a) => { println!("[CURL] DNS: '{}' -> [{}] (registro AAAA recibido)", host, portix_rt::net::fmt_ipv6(&a)); dest_ip6 = Some(a); }
            None => { println!("[CURL-DNS-FAIL] sin registro AAAA para '{}'", host); return 1; }
        }
    }
    let dest_ip = if let Some(ip) = parse_ipv4(host) {
        ip
    } else if dest_ip6.is_some() {
        [0, 0, 0, 0]
    } else {
        println!("[CURL] Resolviendo DNS para host '{}' via UDP 10.0.2.3:53...", host);
        match resolve_dns(&sock, host, [10, 0, 2, 3]) {
            Some(ip) => { println!("[CURL] DNS: '{}' -> {}.{}.{}.{} (registro A recibido)", host, ip[0], ip[1], ip[2], ip[3]); ip }
            None => { println!("[CURL-DNS-FAIL] sin respuesta A para '{}'", host); return 1; }
        }
    };

    let mut req = String::from("GET ");
    req.push_str(path);
    req.push_str(" HTTP/1.1\r\nHost: ");
    req.push_str(host);
    req.push_str("\r\nUser-Agent: Portix-cURL/1.0 (x86_64; Rust)\r\nConnection: close\r\n\r\n");

    // ─────────────────────────────────────────────────────────────────────
    //  [FRENTE B] HTTP/1.1 REAL sobre TCP del kernel: connect (handshake
    //  esperado), GET, lectura hasta FIN, cabeceras parseadas y cuerpo
    //  guardado tal cual llego por la red. Se elimino la "descarga" que leia
    //  /var/pkg/repo/<archivo> tras mandar un datagrama UDP.
    // ─────────────────────────────────────────────────────────────────────
    drop(sock);
    let (fd, rc) = if let Some(ip6) = dest_ip6 {
        // [IPv6 REAL] socket AF_INET6 + sockaddr_in6: TCP sobre IPv6 del kernel
        println!("[CURL] Conectando por TCP/IPv6 a [{}]:{}...", portix_rt::net::fmt_ipv6(&ip6), port);
        let fd = portix_rt::syscall::sys_socket(portix_rt::net::AF_INET6, portix_rt::net::SOCK_STREAM, 0) as i32;
        if fd < 0 { println!("curl: fallo al crear socket TCP IPv6 (err={})", fd); return 1; }
        let addr = portix_rt::net::SockAddrIn6::new(ip6, port);
        (fd, portix_rt::syscall::sys_connect(fd, &addr as *const _ as *const u8, core::mem::size_of::<portix_rt::net::SockAddrIn6>() as u32))
    } else {
        println!("[CURL] Conectando por TCP a {}.{}.{}.{}:{}...", dest_ip[0], dest_ip[1], dest_ip[2], dest_ip[3], port);
        let fd = portix_rt::syscall::sys_socket(portix_rt::net::AF_INET, portix_rt::net::SOCK_STREAM, 0) as i32;
        if fd < 0 { println!("curl: fallo al crear socket TCP (err={})", fd); return 1; }
        let addr = portix_rt::net::SockAddrIn::new(dest_ip, port);
        (fd, portix_rt::syscall::sys_connect(fd, &addr as *const _ as *const u8, core::mem::size_of::<portix_rt::net::SockAddrIn>() as u32))
    };
    if rc < 0 {
        println!("[CURL-FAIL] connect() fallo (err={}): sin SYN-ACK del servidor", rc);
        portix_rt::syscall::sys_close(fd);
        return 1;
    }
    println!("[CURL] Conexion TCP establecida (handshake completado)");
    if pausa_ms > 0 {
        println!("[CURL-PAUSA] esperando {} ms antes de enviar la peticion", pausa_ms);
        let ts = portix_rt::syscall::Timespec { tv_sec: (pausa_ms / 1000) as i64, tv_nsec: ((pausa_ms % 1000) * 1_000_000) as i64 };
        let _ = portix_rt::syscall::sys_nanosleep(&ts as *const _, core::ptr::null_mut());
    }

    let mut respuesta: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut lecturas = 0u32;
    if tls_eco {
        return prueba_keyupdate(fd, host);
    }
    if is_https {
        // [FRENTE B-2] TLS 1.3 REAL (portix_rt::tls13): X25519, HKDF-SHA256,
        // ChaCha20-Poly1305, CertificateVerify RSA y cadena X.509 validada
        // contra /etc/ssl/certs con el reloj real. Si algo no verifica, se
        // aborta con el motivo; no hay "canal cifrado" fingido.
        let roots = portix_rt::tls13::x509::load_system_roots();
        let now = portix_rt::tls13::now_epoch();
        println!("[CURL] Iniciando TLS 1.3 con '{}' ({} raices de confianza, epoch={})", host, roots.len(), now);
        let mut log = |m: &str| println!("{}", m);
        let identidad = if cert_cliente {
            let (cp, kp) = match cert_tipo { "rsa" => ("/etc/ssl/certs/client-rsa.crt", "/etc/ssl/private/client-rsa.key"), "ed25519" => ("/etc/ssl/certs/client-ed25519.crt", "/etc/ssl/private/client-ed25519.key"), _ => ("/etc/ssl/certs/client.crt", "/etc/ssl/private/client.key") };
            let c = portix_rt::fs::read_to_string(cp).unwrap_or_default();
            let k = portix_rt::fs::read_to_string(kp).unwrap_or_default();
            match portix_rt::tls13::Identity::from_pem(&c, &k) {
                Ok(id) => { println!("[CURL] Identidad de cliente cargada: CN='{}' ({} cert, {}; autocomprobacion firma/verificacion y correspondencia con el certificado OK)", id.leaf_cn(), id.chain_der.len(), id.key.describe()); Some(id) }
                Err(e) => { println!("[CURL-TLS-FAIL] certificado de cliente: {}", e); portix_rt::syscall::sys_close(fd); return 1; }
            }
        } else { None };
        let mut tls = match portix_rt::tls13::TlsClient::handshake_full(fd, host, &roots, now, None, identidad.as_ref(), &mut log) {
            Ok(t) => t,
            Err(e) => { println!("[CURL-TLS-FAIL] {}", e); portix_rt::syscall::sys_close(fd); return 1; }
        };
        println!("[CURL-TLS-OK] canal TLS 1.3 verificado: {} / {} / raiz '{}'{}", tls.cipher, tls.server_sig, tls.trust_anchor,
            if tls.client_authenticated { alloc::format!(" / cliente autenticado (mTLS, {})", tls.client_sig) } else { String::new() });
        if let Err(e) = tls.write(req.as_bytes()) { println!("[CURL-TLS-FAIL] envio: {}", e); portix_rt::syscall::sys_close(fd); return 1; }
        println!("[CURL] Peticion HTTP enviada cifrada ({} bytes)", req.len());
        loop {
            // HTTP/1.1 keep-alive: si ya tenemos cabeceras + Content-Length completo, no hay que
            // esperar al cierre del servidor (antes se agotaban 6 EAGAIN y se avisaba de un fallo falso).
            if respuesta_completa(&respuesta) { break; }
            match tls.read() {
                Ok(d) if d.is_empty() => break,
                Ok(d) => { respuesta.extend_from_slice(&d); lecturas += 1; }
                Err(e) => { println!("[CURL-TLS-FAIL] lectura: {}", e); break; }
            }
        }
        tls.close();
        if reanudar {
            portix_rt::syscall::sys_close(fd);
            return reanudar_sesion(dest_ip, port, host, &roots, tls.ticket.clone(), req.as_bytes());
        }
    } else {
        let enviado = portix_rt::syscall::sys_write(fd, req.as_ptr(), req.len());
        if enviado < 0 { println!("[CURL-FAIL] write() fallo (err={})", enviado); portix_rt::syscall::sys_close(fd); return 1; }
        println!("[CURL] Peticion HTTP enviada ({} bytes)", enviado);
        if lector_lento {
            // Prueba de control de flujo: no leer durante 1,5 s para que el bufer de
            // recepcion del nucleo se llene y la ventana anunciada llegue a cero.
            println!("[CURL] lector lento: sin leer del socket durante 1500 ms (la ventana de recepcion debe cerrarse)");
            let ts = portix_rt::syscall::Timespec { tv_sec: 1, tv_nsec: 500_000_000 };
            let _ = portix_rt::syscall::sys_nanosleep(&ts as *const _, core::ptr::null_mut());
        }
        let mut chunk = [0u8; 4096];
        loop {
            let n = portix_rt::syscall::sys_read(fd, chunk.as_mut_ptr(), chunk.len());
            if n > 0 { respuesta.extend_from_slice(&chunk[..n as usize]); lecturas += 1; continue; }
            if n == 0 { break; }                 // FIN del servidor: fin de la respuesta
            if n == -11 && lecturas == 0 { continue; } // aun sin datos: reintentar
            break;                               // -EAGAIN tras datos o error: dar por terminada
        }
    }
    portix_rt::syscall::sys_close(fd);
    println!("[CURL] Respuesta recibida: {} bytes en {} lecturas", respuesta.len(), lecturas);

    // Cabeceras HTTP
    let sep = match respuesta.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(p) => p,
        None => { println!("[CURL-FAIL] respuesta sin cabeceras HTTP"); return 1; }
    };
    let cab = core::str::from_utf8(&respuesta[..sep]).unwrap_or("");
    let linea = cab.lines().next().unwrap_or("");
    let estado = linea.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    let content_length = cab.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.eq_ignore_ascii_case("content-length") { v.trim().parse::<usize>().ok() } else { None }
    });
    println!("[CURL] {} (Content-Length: {})", linea, content_length.map(|v| alloc::format!("{}", v)).unwrap_or_else(|| String::from("-")));
    if estado != 200 {
        println!("[CURL-FAIL] HTTP {}: no se guarda nada", estado);
        return 1;
    }
    let cuerpo = &respuesta[sep + 4..];
    if let Some(cl) = content_length {
        if cuerpo.len() != cl {
            println!("[CURL-FAIL] cuerpo incompleto: {} de {} bytes", cuerpo.len(), cl);
            return 1;
        }
        println!("[CURL] cuerpo completo: {}/{} bytes", cuerpo.len(), cl);
    }
    if let Some(ref out) = out_file {
        if let Err(e) = portix_rt::fs::write(out, cuerpo) { println!("[CURL-FAIL] no se pudo escribir '{}' (errno {})", out, -e); return 1; }
        println!("[CURL] Archivo descargado guardado exitosamente en '{}' ({} bytes)", out, cuerpo.len());
    } else {
        if let Ok(t) = core::str::from_utf8(cuerpo) { print!("{}", t); } else { println!("[CURL] cuerpo binario de {} bytes (use -o)", cuerpo.len()); }
    }
    0
}


fn conectar_tcp(dest_ip: [u8; 4], port: u16) -> Result<i32, i64> {
    let fd = portix_rt::syscall::sys_socket(portix_rt::net::AF_INET, portix_rt::net::SOCK_STREAM, 0) as i32;
    if fd < 0 { return Err(fd as i64); }
    let addr = portix_rt::net::SockAddrIn::new(dest_ip, port);
    let rc = portix_rt::syscall::sys_connect(fd, &addr as *const _ as *const u8, core::mem::size_of::<portix_rt::net::SockAddrIn>() as u32);
    if rc < 0 { portix_rt::syscall::sys_close(fd); return Err(rc as i64); }
    Ok(fd)
}

/// Segunda conexion TLS 1.3 ofreciendo el ticket PSK de la primera (RFC 8446 §4.6.1 / §2.2).
fn reanudar_sesion(dest_ip: [u8; 4], port: u16, host: &str, roots: &[portix_rt::tls13::x509::Certificate],
                   ticket: Option<portix_rt::tls13::SessionTicket>, req: &[u8]) -> i32 {
    let t = match ticket { Some(t) => t, None => { println!("[TLS-RESUMED-FAIL] el servidor no emitio NewSessionTicket en la primera conexion"); return 1; } };
    println!("[CURL] Ticket PSK recibido ({} bytes, vigencia {} s); abriendo segunda conexion para reanudar...", t.ticket_len(), t.lifetime);
    let fd = match conectar_tcp(dest_ip, port) { Ok(f) => f, Err(e) => { println!("[TLS-RESUMED-FAIL] connect() fallo (err={})", e); return 1; } };
    let now = portix_rt::tls13::now_epoch();
    let mut log = |m: &str| println!("{}", m);
    let mut tls = match portix_rt::tls13::TlsClient::handshake_with(fd, host, roots, now, Some(&t), &mut log) {
        Ok(t) => t, Err(e) => { println!("[TLS-RESUMED-FAIL] {}", e); portix_rt::syscall::sys_close(fd); return 1; }
    };
    if !tls.resumed { println!("[TLS-RESUMED-FAIL] el servidor rechazo la PSK e hizo handshake completo"); tls.close(); portix_rt::syscall::sys_close(fd); return 1; }
    let _ = tls.write(req);
    let mut total = 0usize;
    loop { match tls.read() { Ok(d) if d.is_empty() => break, Ok(d) => total += d.len(), Err(_) => break } }
    tls.close(); portix_rt::syscall::sys_close(fd);
    if total == 0 { println!("[TLS-RESUMED-FAIL] sesion reanudada pero sin respuesta HTTP"); return 1; }
    println!("[TLS-RESUMED] segunda conexion reanudada con PSK (ticket de {} bytes): {} bytes de respuesta cifrados con claves derivadas de la PSK", t.ticket_len(), total);
    0
}

/// Prueba de KeyUpdate (RFC 8446 §4.6.3) contra un servidor TLS 1.3 de eco
/// (p. ej. `openssl s_server -tls1_3`): se pide rotacion al par, se escribe con
/// la clave nueva y se exige una respuesta descifrable con la clave rotada del par.
fn prueba_keyupdate(fd: i32, host: &str) -> i32 {
    let roots = portix_rt::tls13::x509::load_system_roots();
    let now = portix_rt::tls13::now_epoch();
    let mut log = |m: &str| println!("{}", m);
    let mut tls = match portix_rt::tls13::TlsClient::handshake(fd, host, &roots, now, &mut log) {
        Ok(t) => t, Err(e) => { println!("[TLS-KEYUPDATE-FAIL] handshake: {}", e); portix_rt::syscall::sys_close(fd); return 1; }
    };
    println!("[CURL-TLS-OK] canal TLS 1.3 verificado: {} / {} / raiz '{}'", tls.cipher, tls.server_sig, tls.trust_anchor);
    if let Err(e) = tls.write(b"antes-del-keyupdate\n") { println!("[TLS-KEYUPDATE-FAIL] escritura inicial: {}", e); return 1; }
    if let Err(e) = tls.key_update(true) { println!("[TLS-KEYUPDATE-FAIL] envio de KeyUpdate: {}", e); return 1; }
    println!("[TLS] KeyUpdate(update_requested) enviado; clave de escritura rotada (traffic upd)");
    if let Err(e) = tls.write(b"despues-del-keyupdate\n") { println!("[TLS-KEYUPDATE-FAIL] escritura tras rotar: {}", e); return 1; }
    let mut recibido = alloc::vec::Vec::new();
    loop {
        match tls.read() {
            Ok(d) if d.is_empty() => break,
            Ok(d) => { recibido.extend_from_slice(&d); if recibido.windows(3).any(|w| w == b"fin") { break; } }
            Err(e) => { println!("[TLS-KEYUPDATE-FAIL] lectura: {}", e); portix_rt::syscall::sys_close(fd); return 1; }
        }
    }
    let (tx, rx) = tls.key_updates();
    tls.close(); portix_rt::syscall::sys_close(fd);
    if rx == 0 { println!("[TLS-KEYUPDATE-FAIL] el par no respondio con su KeyUpdate (tx={}, rx={})", tx, rx); return 1; }
    println!("[TLS-KEYUPDATE-OK] rotacion de claves verificada en ambos sentidos: KeyUpdate tx={} rx={}; {} bytes descifrados con la clave rotada del servidor: {:?}",
        tx, rx, recibido.len(), core::str::from_utf8(&recibido).unwrap_or("?").trim());
    0
}
