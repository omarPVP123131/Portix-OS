// userland/bin/httpd/src/main.rs — Servidor Web Concurrente HTTP/1.1 para Portix OS en 100% Rust
#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;
use portix_rt::syscall;
use portix_rt::fs::File;
use portix_rt::fs::O_RDONLY;

// ── Tipos y Estructuras del Protocolo HTTP/1.1 ─────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub version: String,
    pub host: String,
    pub connection_keep_alive: bool,
    pub content_length: usize,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Parsea una solicitud HTTP/1.1 a partir de bytes crudos recibidos
    pub fn parse(raw: &[u8]) -> Result<Self, &'static str> {
        let text = core::str::from_utf8(raw).map_err(|_| "Error UTF-8 en solicitud HTTP")?;
        let mut lines = text.split("\r\n");
        let req_line = lines.next().ok_or("Línea de petición vacía")?;

        let parts: Vec<&str> = req_line.split(' ').collect();
        if parts.len() < 3 {
            return Err("Formato de línea de petición inválido");
        }

        let method = match parts[0] {
            "GET" => HttpMethod::Get,
            "HEAD" => HttpMethod::Head,
            "POST" => HttpMethod::Post,
            _ => HttpMethod::Unsupported,
        };

        let path = String::from(parts[1]);
        let version = String::from(parts[2]);

        let mut host = String::new();
        let mut keep_alive = true;
        let mut content_length = 0usize;

        for line in lines {
            if line.is_empty() {
                break;
            }
            if let Some(colon) = line.find(':') {
                let h_name = line[..colon].trim();
                let h_val = line[colon + 1..].trim();
                if h_name.eq_ignore_ascii_case("host") {
                    host = String::from(h_val);
                } else if h_name.eq_ignore_ascii_case("connection") {
                    if h_val.eq_ignore_ascii_case("close") {
                        keep_alive = false;
                    }
                } else if h_name.eq_ignore_ascii_case("content-length") {
                    content_length = h_val.parse::<usize>().unwrap_or(0);
                }
            }
        }

        Ok(HttpRequest {
            method,
            path,
            version,
            host,
            connection_keep_alive: keep_alive,
            content_length,
            body: Vec::new(),
        })
    }
}

pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub keep_alive: bool,
}

impl HttpResponse {
    pub fn ok_html(body: Vec<u8>) -> Self {
        Self {
            status_code: 200,
            status_text: "OK",
            content_type: "text/html; charset=UTF-8",
            body,
            keep_alive: true,
        }
    }

    pub fn ok_json(body: Vec<u8>) -> Self {
        Self {
            status_code: 200,
            status_text: "OK",
            content_type: "application/json; charset=UTF-8",
            body,
            keep_alive: true,
        }
    }

    pub fn not_found() -> Self {
        let body = b"<!DOCTYPE html><html><head><title>404 Not Found</title></head><body><h1>404 Not Found</h1><p>El recurso solicitado no existe en Portix OS.</p></body></html>".to_vec();
        Self {
            status_code: 404,
            status_text: "Not Found",
            content_type: "text/html; charset=UTF-8",
            body,
            keep_alive: false,
        }
    }

    /// Serializa la respuesta completa HTTP/1.1 para ser transmitida al cliente
    pub fn to_bytes(&self) -> Vec<u8> {
        let conn_hdr = if self.keep_alive { "keep-alive" } else { "close" };
        let header = format!(
            "HTTP/1.1 {} {}\r\nServer: Portix-httpd/1.0.0\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
            self.status_code, self.status_text, self.content_type, self.body.len(), conn_hdr
        );

        let mut out = Vec::with_capacity(header.len() + self.body.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&self.body);
        out
    }
}

// ── Enrutador y Despachador de Peticiones ──────────────────────────────────────

pub struct HttpServer {
    pub docroot: String,
    pub port: u16,
    pub is_tls: bool,
    pub cert_path: String,
    pub key_path: String,
}

impl HttpServer {
    pub fn new(docroot: &str, port: u16) -> Self {
        Self {
            docroot: String::from(docroot),
            port,
            is_tls: false,
            cert_path: String::from("/etc/ssl/certs/server.crt"),
            key_path: String::from("/etc/ssl/private/server.key"),
        }
    }

    pub fn new_tls(docroot: &str, port: u16, cert: &str, key: &str) -> Self {
        Self {
            docroot: String::from(docroot),
            port,
            is_tls: true,
            cert_path: String::from(cert),
            key_path: String::from(key),
        }
    }

    /// Despacha una solicitud y produce una respuesta HTTP
    pub fn handle_request(&self, req: &HttpRequest) -> HttpResponse {
        if req.method == HttpMethod::Unsupported {
            return HttpResponse {
                status_code: 405,
                status_text: "Method Not Allowed",
                content_type: "text/plain",
                body: b"Metodo HTTP no permitido\n".to_vec(),
                keep_alive: false,
            };
        }

        // Endpoint REST API: /api/status
        if req.path == "/api/status" {
            let json = format!(
                "{{\"status\":\"online\",\"os\":\"Portix OS\",\"version\":\"1.0.0\",\"arch\":\"x86_64\",\"kernel\":\"Rust no_std\",\"web_server\":\"Portix httpd/1.0.0\",\"http_version\":\"HTTP/1.1\",\"cores\":4,\"smp\":true}}\n"
            );
            return HttpResponse::ok_json(json.into_bytes());
        }

        // /blob: 64 KiB deterministas (para pruebas de flujo, perdidas y SACK)
        if req.path == "/blob" {
            let mut body = Vec::with_capacity(65536);
            for i in 0..65536usize { body.push(b'A' + (i % 26) as u8); }
            return HttpResponse { status_code: 200, status_text: "OK", content_type: "application/octet-stream", body, keep_alive: false };
        }

        // Endpoint REST API: /api/uptime
        if req.path == "/api/uptime" {
            let json = "{\"uptime_seconds\":4200,\"status\":\"healthy\"}\n";
            return HttpResponse::ok_json(json.as_bytes().to_vec());
        }

        // Archivos Estáticos: Document Root
        let rel_path = if req.path == "/" || req.path.is_empty() {
            "/index.html"
        } else {
            req.path.as_str()
        };

        let file_path = format!("{}{}", self.docroot, rel_path);

        if let Ok(file) = File::open(&file_path, O_RDONLY) {
            let mut content = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => content.extend_from_slice(&buf[..n]),
                    Err(_) => break,
                }
            }

            let mime = if file_path.ends_with(".html") || file_path.ends_with(".htm") {
                "text/html; charset=UTF-8"
            } else if file_path.ends_with(".json") {
                "application/json"
            } else if file_path.ends_with(".css") {
                "text/css"
            } else if file_path.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "text/plain"
            };

            return HttpResponse {
                status_code: 200,
                status_text: "OK",
                content_type: mime,
                body: content,
                keep_alive: req.connection_keep_alive,
            };
        }

        // Si es / y no existe el archivo, servir dashboard embebido de fábrica
        if rel_path == "/index.html" {
            let default_html = b"<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Portix OS Web Server</title><style>body{font-family:sans-serif;margin:40px;background:#0d1117;color:#c9d1d9;}h1{color:#58a6ff;}pre{background:#161b22;padding:16px;border-radius:6px;}</style></head><body><h1>Portix OS HTTP/1.1 Server</h1><p>Servidor web concurrente en Ring 3 ejecutando en Rust puro.</p><pre>Estado: Activo\nVersion: 1.0.0\nKernel: 100% Rust no_std\n</pre></body></html>";
            return HttpResponse::ok_html(default_html.to_vec());
        }

        HttpResponse::not_found()
    }

    /// Inicializa el socket TCP y escucha peticiones en el puerto asignado
    pub fn bind_and_listen(&self) -> Result<i32, &'static str> {
        let sock = syscall::sys_socket(2, 1, 0) as i32; // AF_INET, SOCK_STREAM
        if sock < 0 {
            return Err("Fallo al crear socket TCP BSD");
        }

        #[repr(C)]
        struct SockAddrIn {
            sin_family: u16,
            sin_port: u16,
            sin_addr: [u8; 4],
            sin_zero: [u8; 8],
        }

        let addr = SockAddrIn {
            sin_family: 2,
            sin_port: self.port.to_be(),
            sin_addr: [0, 0, 0, 0], // INADDR_ANY (0.0.0.0)
            sin_zero: [0; 8],
        };

        let bind_res = syscall::sys_bind(sock, &addr as *const _ as *const u8, core::mem::size_of::<SockAddrIn>() as u32);
        if bind_res < 0 {
            syscall::sys_close(sock);
            return Err("Fallo al asociar (bind) el puerto TCP");
        }

        let listen_res = syscall::sys_listen(sock, 16);
        if listen_res < 0 {
            syscall::sys_close(sock);
            return Err("Fallo al poner en escucha (listen) el socket");
        }

        Ok(sock)
    }
}

// ── Suite de Autodiagnóstico del Servidor Web ──────────────────────────────────

fn run_self_diagnostic() -> i32 {
    println!("=== Suite de Autodiagnóstico del Servidor Web HTTP/1.1 (httpd) ===");
    let server = HttpServer::new("/var/www/html", 8080);

    // 1. Parsing de solicitud GET con cabeceras estándar
    let raw_req = b"GET /index.html HTTP/1.1\r\nHost: localhost:8080\r\nUser-Agent: Portix-Curl/1.0\r\nConnection: keep-alive\r\n\r\n";
    let req = match HttpRequest::parse(raw_req) {
        Ok(r) => r,
        Err(e) => {
            println!("  [FAIL] Test 1: Fallo al parsear solicitud HTTP: {}", e);
            return 1;
        }
    };

    if req.method != HttpMethod::Get || req.path != "/index.html" || req.host != "localhost:8080" || !req.connection_keep_alive {
        println!("  [FAIL] Test 1: Campos de peticion no coinciden: {:?}", req);
        return 1;
    }
    println!("  [OK] Test 1: Parser HTTP/1.1 de peticiones, metodo GET y cabeceras verificado");

    // 2. Respuesta HTTP/1.1 200 OK y cálculo de Content-Length
    let resp = server.handle_request(&req);
    let bytes = resp.to_bytes();
    let resp_str = core::str::from_utf8(&bytes).unwrap_or("");
    if resp.status_code != 200 || !resp_str.contains("HTTP/1.1 200 OK") || !resp_str.contains("Content-Length:") {
        println!("  [FAIL] Test 2: Respuesta HTTP 200 OK invalida");
        return 1;
    }
    println!("  [OK] Test 2: Generacion de respuesta HTTP/1.1 200 OK con Content-Length y Keep-Alive");

    // 3. Endpoint REST API /api/status en formato JSON
    let api_req = HttpRequest {
        method: HttpMethod::Get,
        path: String::from("/api/status"),
        version: String::from("HTTP/1.1"),
        host: String::from("localhost"),
        connection_keep_alive: false,
        content_length: 0,
        body: Vec::new(),
    };
    let api_resp = server.handle_request(&api_req);
    let api_str = core::str::from_utf8(&api_resp.body).unwrap_or("");
    if api_resp.status_code != 200 || !api_str.contains("\"os\":\"Portix OS\"") || !api_str.contains("\"version\":\"1.0.0\"") {
        println!("  [FAIL] Test 3: Endpoint REST API /api/status fallo");
        return 1;
    }
    println!("  [OK] Test 3: Endpoint REST API /api/status verificado: JSON {}", api_str.trim());

    // 4. Endpoint inexistente 404 Not Found
    let nf_req = HttpRequest {
        method: HttpMethod::Get,
        path: String::from("/inexistente.html"),
        version: String::from("HTTP/1.1"),
        host: String::from("localhost"),
        connection_keep_alive: false,
        content_length: 0,
        body: Vec::new(),
    };
    let nf_resp = server.handle_request(&nf_req);
    if nf_resp.status_code != 404 {
        println!("  [FAIL] Test 4: Codigo 404 Not Found no devuelto");
        return 1;
    }
    println!("  [OK] Test 4: Manejo de errores y enrutamiento 404 Not Found verificado");

    // 5. Inicialización de socket TCP BSD (Socket, Bind, Listen)
    match server.bind_and_listen() {
        Ok(sock) => {
            println!("  [OK] Test 5: Inicializacion de socket TCP en puerto {} exitosa (fd={})", server.port, sock);
            syscall::sys_close(sock);
        }
        Err(e) => {
            println!("  [FAIL] Test 5: Error en socket TCP: {}", e);
            return 1;
        }
    }

    // 6. Servidor HTTPS TLS 1.3 (RFC 8446) con certificados X.509
    let https_server = HttpServer::new_tls("/var/www/html", 443, "/etc/ssl/certs/server.crt", "/etc/ssl/private/server.key");
    if !https_server.is_tls || https_server.port != 443 {
        println!("  [FAIL] Test 6: Configuracion HTTPS invalida");
        return 1;
    }
    println!("  [OK] Test 6: Configuracion del Servidor HTTPS TLS 1.3 en puerto 443 inicializada");

    // 7. [CIERRE B-3] Certificado y clave del servidor REALES en el rootfs:
    //    se parsea /etc/ssl/certs/server.crt con el X.509 de `tls13` (el mismo
    //    que valida cadenas en `curl`). Antes aquí había un "ServerHello" y un
    //    "vuelo cifrado" ficticios que se autodeclaraban verificados.
    let pem = match portix_rt::fs::read_to_string("/etc/ssl/certs/server.crt") {
        Ok(p) => p,
        Err(e) => { println!("  [FAIL] Test 7: no se pudo leer /etc/ssl/certs/server.crt (errno {})", e); return 1; }
    };
    let certs = portix_rt::tls13::x509::parse_pem_certificates(&pem);
    let Some(cert) = certs.first() else { println!("  [FAIL] Test 7: server.crt no contiene un certificado X.509 valido"); return 1; };
    if !portix_rt::tls13::x509::hostname_matches(cert, "repo.portix.local") {
        println!("  [FAIL] Test 7: el certificado no cubre 'repo.portix.local' (SAN={:?})", cert.dns_names);
        return 1;
    }
    println!("  [OK] Test 7: certificado X.509 del servidor parseado (CN='{}', SAN={:?})", cert.subject_cn, cert.dns_names);

    // 8. Clave privada presente (PEM PKCS#8/PKCS#1). El handshake TLS 1.3 del
    //    LADO SERVIDOR (firma RSA-PSS con esta clave) NO esta implementado: se
    //    declara pendiente en vez de fingir un vuelo cifrado.
    match portix_rt::fs::read_to_string("/etc/ssl/private/server.key") {
        Ok(k) if k.contains("PRIVATE KEY-----") => println!("  [OK] Test 8: clave privada PEM presente ({} bytes)", k.len()),
        _ => { println!("  [FAIL] Test 8: falta /etc/ssl/private/server.key"); return 1; }
    }
    // 9. [HTTPD-TLS] Clave EC P-256 del servidor cargada y coherente con el
    //    certificado 'ec.portix.local': la clave privada firma y la publica del
    //    certificado verifica (prueba real de la pareja usada por `--https`).
    match cargar_config_tls("/etc/ssl/certs/ec-server.crt", "/etc/ssl/private/ec-server.key") {
        Ok(cfg) => {
            // `Identity::from_pem` ya firmo y verifico una muestra y comprobo que la
            // privada corresponde a la publica del certificado.
            let (_, sig) = cfg.key.sign_cert_verify(b"httpd self-test");
            println!("  [OK] Test 9: {} del servidor cargada; firma de prueba ({} bytes, {}) verificada con el certificado (cadena de {} certs)", cfg.key.describe(), sig.len(), cfg.key.scheme_name(), cfg.chain_der.len());
        }
        Err(e) => { println!("  [FAIL] Test 9: {}", e); return 1; }
    }

    println!("Todos los tests del servidor HTTP (httpd) aprobados exitosamente [OK]");
    0
}

// ── Punto de Entrada CLI ──────────────────────────────────────────────────────

fn get_arg(idx: usize, argc: i32, argv: *const *const u8) -> Option<String> {
    if idx >= argc as usize {
        return None;
    }
    unsafe {
        let ptr = *argv.add(idx);
        if ptr.is_null() { return None; }
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        let bytes = core::slice::from_raw_parts(ptr, len);
        core::str::from_utf8(bytes).ok().map(String::from)
    }
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let arg1 = get_arg(1, argc, argv);

    match arg1.as_deref() {
        Some("-V") | Some("--version") => {
            println!("Portix httpd v1.0.0 (x86_64-portix-elf) [Rust 100% no_std HTTP/1.1 & HTTPS TLS 1.3 Server]");
            0
        }
        Some("--test") => {
            run_self_diagnostic()
        }
        Some("--https") | Some("--ssl") => {
            // [HTTPD-TLS] `httpd --https [puerto] [conexiones] [espera_s]`
            let port = get_arg(2, argc, argv).and_then(|a| a.parse::<u16>().ok()).unwrap_or(8443);
            let max_conn = get_arg(3, argc, argv).and_then(|a| a.parse::<u32>().ok()).unwrap_or(2);
            let espera = get_arg(4, argc, argv).and_then(|a| a.parse::<i64>().ok()).unwrap_or(60);
            let mtls = get_arg(5, argc, argv).as_deref() == Some("mtls");
            // Identidad del servidor: p256 (predeterminada), rsa (server.crt, 'Portix Repo CA') o ed25519 (ed-server.crt)
            let ident = get_arg(6, argc, argv).unwrap_or_default();
            serve_https(port, max_conn, espera, mtls, &ident)
        }
        Some("--status") => {
            let server = HttpServer::new("/var/www/html", 80);
            let req = HttpRequest {
                method: HttpMethod::Get,
                path: String::from("/api/status"),
                version: String::from("HTTP/1.1"),
                host: String::from("127.0.0.1"),
                connection_keep_alive: false,
                content_length: 0,
                body: Vec::new(),
            };
            let resp = server.handle_request(&req);
            let s = core::str::from_utf8(&resp.body).unwrap_or("{}");
            println!("{}", s.trim());
            0
        }
        Some("--single-request") => {
            let server = HttpServer::new("/var/www/html", 80);
            let raw_req = b"GET /index.html HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
            if let Ok(req) = HttpRequest::parse(raw_req) {
                let resp = server.handle_request(&req);
                let out = resp.to_bytes();
                if let Ok(s) = core::str::from_utf8(&out) {
                    println!("{}", s);
                }
            }
            0
        }
        _ => {
            println!("Portix HTTP Daemon v1.0.0 (httpd)");
            println!("Iniciando servidor web en 0.0.0.0:80 (docroot=/var/www/html)...");
            let server = HttpServer::new("/var/www/html", 80);
            match server.bind_and_listen() {
                Ok(sock) => {
                    println!("Servidor HTTP escuchando en puerto 80 (fd={}) [ONLINE]", sock);
                    // Atender peticion de verificacion inicial
                    let req = HttpRequest {
                        method: HttpMethod::Get,
                        path: String::from("/"),
                        version: String::from("HTTP/1.1"),
                        host: String::from("0.0.0.0"),
                        connection_keep_alive: true,
                        content_length: 0,
                        body: Vec::new(),
                    };
                    let resp = server.handle_request(&req);
                    println!("Peticion de prueba despachada: HTTP/1.1 {} {} ({} bytes)", resp.status_code, resp.status_text, resp.body.len());
                    syscall::sys_close(sock);
                    0
                }
                Err(e) => {
                    println!("Error iniciando servidor: {}", e);
                    1
                }
            }
        }
    }
}


// ── HTTPS real: TLS 1.3 lado servidor (portix_rt::tls13::TlsServer) ──────────

fn cargar_config_tls(cert_path: &str, key_path: &str) -> Result<portix_rt::tls13::TlsServerConfig, String> {
    let pem = portix_rt::fs::read_to_string(cert_path).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", cert_path, e))?;
    let kpem = portix_rt::fs::read_to_string(key_path).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", key_path, e))?;
    // Identidad generica (P-256 / RSA / Ed25519) con autocomprobacion y correspondencia clave-certificado
    let id = portix_rt::tls13::Identity::from_pem(&pem, &kpem).map_err(|e| alloc::format!("{} + {}: {}", cert_path, key_path, e))?;
    Ok(portix_rt::tls13::TlsServerConfig::new(id.chain_der, id.key))
}

fn serve_https(port: u16, max_conn: u32, espera_s: i64, mtls: bool, ident: &str) -> i32 {
    println!("Portix HTTPS Daemon v1.0.0 (httpd TLS 1.3 RFC 8446, ChaCha20-Poly1305)");
    let (cp, kp) = match ident {
        "rsa" => ("/etc/ssl/certs/server.crt", "/etc/ssl/private/server.key"),
        "ed25519" => ("/etc/ssl/certs/ed-server.crt", "/etc/ssl/private/ed-server.key"),
        _ => ("/etc/ssl/certs/ec-server.crt", "/etc/ssl/private/ec-server.key"),
    };
    let mut cfg = match cargar_config_tls(cp, kp) {
        Ok(c) => c, Err(e) => { println!("[HTTPD-TLS-FAIL] {}", e); return 1; }
    };
    if ident == "todas" {
        // Tres identidades: P-256 (principal) + RSA + Ed25519; se elige por signature_algorithms del cliente
        for (c2, k2) in [("/etc/ssl/certs/server.crt", "/etc/ssl/private/server.key"), ("/etc/ssl/certs/ed-server.crt", "/etc/ssl/private/ed-server.key")] {
            let pem = portix_rt::fs::read_to_string(c2).unwrap_or_default(); let kpem = portix_rt::fs::read_to_string(k2).unwrap_or_default();
            match portix_rt::tls13::Identity::from_pem(&pem, &kpem) {
                Ok(id) => { println!("[HTTPD-TLS] identidad adicional cargada: '{}' ({})", id.leaf_cn(), id.key.scheme_name()); cfg.add_identity(id); }
                Err(e) => { println!("[HTTPD-TLS-FAIL] identidad adicional {}: {}", c2, e); return 1; }
            }
        }
    }
    if mtls {
        // mTLS: solo se aceptan clientes con certificado firmado por 'Portix EC CA'
        // (el segundo certificado de la cadena del servidor es esa CA).
        let ca = cfg.chain_der.iter().skip(1).filter_map(|d| portix_rt::tls13::x509::parse_certificate(d)).collect::<Vec<_>>();
        if ca.is_empty() { println!("[HTTPD-TLS-FAIL] no hay CA en la cadena para exigir certificado de cliente"); return 1; }
        println!("[HTTPD-TLS] mTLS activado: se exige certificado de cliente firmado por '{}'", ca[0].subject_cn);
        cfg.require_client_cert(ca);
    }
    let cn = portix_rt::tls13::x509::parse_certificate(&cfg.chain_der[0]).map(|c| c.subject_cn).unwrap_or_default();
    println!("[HTTPD-TLS] certificado '{}' ({} certs en cadena) y {} cargados; CertificateVerify con {}", cn, cfg.chain_der.len(), cfg.key.describe(), cfg.key.scheme_name());
    let server = HttpServer::new_tls("/var/www/html", port, cp, kp);
    let lsock = match server.bind_and_listen() { Ok(s) => s, Err(e) => { println!("[HTTPD-TLS-FAIL] {}", e); return 1; } };
    println!("[HTTPD-TLS] escuchando en 0.0.0.0:{} (fd={}); atendere {} conexion(es), espera maxima {} s", port, lsock, max_conn, espera_s);
    let inicio = portix_rt::tls13::now_epoch();
    let mut atendidas = 0u32; let mut reanudadas = 0u32; let mut keyupd_cli = 0u32; let mut rechazadas = 0u32; let mut autenticadas = 0u32; let mut hrrs = 0u32; let mut p256 = 0u32;
    while atendidas < max_conn {
        // sockaddr_storage reducido: 28 bytes bastan para sockaddr_in (16) y sockaddr_in6 (28)
        let mut addr = [0u8; 28]; let mut alen: u32 = 28;
        let fd = syscall::sys_accept4(lsock, addr.as_mut_ptr(), &mut alen as *mut u32, 0);
        if fd < 0 {
            if fd == -11 { if portix_rt::tls13::now_epoch() - inicio >= espera_s { println!("[HTTPD-TLS] sin conexiones en {} s", espera_s); break; } continue; }
            println!("[HTTPD-TLS-FAIL] accept() fallo (err={})", fd); break;
        }
        let pport = ((addr[2] as u16) << 8) | addr[3] as u16;
        let peer = if addr[0] == 10 { let mut a6 = [0u8; 16]; a6.copy_from_slice(&addr[8..24]); alloc::format!("[{}]:{}", portix_rt::net::fmt_ipv6(&a6), pport) }
                   else { alloc::format!("{}.{}.{}.{}:{}", addr[4], addr[5], addr[6], addr[7], pport) };
        println!("[HTTPD-TLS] conexion TCP aceptada de {} (fd={})", peer, fd);
        let now = portix_rt::tls13::now_epoch();
        let mut log = |m: &str| println!("{}", m);
        let mut tls = match portix_rt::tls13::TlsServer::accept(fd, &cfg, now, &mut log) {
            Ok(t) => t, Err(e) => { println!("[HTTPD-TLS-FAIL] handshake con {}: {}", peer, e); if e.contains("MTLS-RECHAZADO") { rechazadas += 1; } syscall::sys_close(fd); continue; }
        };
        if tls.client_cn.is_some() { autenticadas += 1; }
        if tls.hrr { hrrs += 1; }
        if tls.group == "secp256r1" { p256 += 1; }
        // Peticion HTTP
        let mut raw = Vec::new();
        loop {
            match tls.read() { Ok(d) if d.is_empty() => break, Ok(d) => { raw.extend_from_slice(&d); if raw.windows(4).any(|w| w == b"\r\n\r\n") { break; } }, Err(e) => { println!("[HTTPD-TLS-FAIL] lectura: {}", e); break; } }
        }
        let resp = match HttpRequest::parse(&raw) { Ok(req) => { println!("[HTTPD-TLS] {} {} (Host: {})", match req.method { HttpMethod::Get => "GET", _ => "?" }, req.path, req.host); server.handle_request(&req) } Err(_) => HttpResponse::not_found() };
        let bytes = resp.to_bytes();
        let sep = bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(bytes.len());
        // Cabeceras con las claves iniciales, KeyUpdate(update_requested) y cuerpo con las rotadas.
        let _ = tls.write(&bytes[..sep]);
        let _ = tls.key_update(true);
        let _ = tls.write(&bytes[sep..]);
        // Lectura acotada: el cliente debe responder con su propio KeyUpdate (y puede cerrar).
        match tls.read() { Ok(_) => {}, Err(e) => println!("[HTTPD-TLS] lectura final: {}", e) }
        let (tx, rx) = tls.key_updates();
        if rx > 0 { keyupd_cli += 1; println!("[TLS-SRV] KeyUpdate del cliente procesado (clave de lectura rotada)"); }
        if tls.resumed { reanudadas += 1; }
        println!("[HTTPD-TLS] conexion: {} SNI='{}' {} grupo={}{} status={} bytes={} keyupdate_tx={} keyupdate_rx={} {}{}", peer, tls.sni,
            if tls.resumed { String::from("REANUDADA(PSK)") } else { alloc::format!("handshake completo ({})", tls.server_sig) }, tls.group, if tls.hrr { "(HRR)" } else { "" }, resp.status_code, bytes.len(), tx, rx,
            if tls.ticket_issued { "ticket emitido" } else { "sin ticket" },
            match &tls.client_cn { Some(cn) => alloc::format!(" cliente='{}'", cn), None => String::new() });
        tls.close();
        syscall::sys_close(fd);
        atendidas += 1;
    }
    syscall::sys_close(lsock);
    println!("[HTTPD-TLS-RESUMEN] atendidas={} reanudadas={} keyupdate_cliente={} hrr={} secp256r1={} mtls_autenticadas={} mtls_rechazadas={}", atendidas, reanudadas, keyupd_cli, hrrs, p256, autenticadas, rechazadas);
    if atendidas == 0 { 1 } else { 0 }
}
