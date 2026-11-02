// kernel/src/net/socket.rs — Sockets Table and Socket Syscalls Backend
#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::arch::Spinlock;
use super::udp::send_udp;
use super::get_ip;

pub const AF_INET: i32 = 2;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;

#[derive(Clone)]
pub struct UdpPacket {
    pub src_ip: super::IpAddr,
    pub src_port: u16,
    pub data: Vec<u8>,
}

pub struct Socket {
    pub domain: i32,
    pub sock_type: i32,
    pub protocol: i32,
    pub local_port: u16,
    pub remote_ip: super::IpAddr,
    pub remote_port: u16,
    pub bound: bool,
    pub connected: bool,
    pub listening: bool,
    pub backlog: i32,
    pub shutdown_rd: bool,
    pub shutdown_wr: bool,
    pub rx_queue: VecDeque<UdpPacket>,
    pub tcp_conn: Option<usize>,
    pub in_use: bool,
    pub ref_count: u32,
    /// [IPv6] error asíncrono pendiente (ICMPv6 destino inalcanzable): lo recoge el siguiente recvfrom.
    pub pending_error: i32,
    /// [CICLO 47-c] AF_UNIX con nombre: ruta `sun_path` a la que esta ligado (bind).
    pub unix_path: [u8; 108],
    pub unix_path_len: u8,
}

impl Socket {
    pub const fn new() -> Self {
        Socket {
            domain: 0,
            sock_type: 0,
            protocol: 0,
            local_port: 0,
            remote_ip: super::IpAddr::UNSPECIFIED4,
            remote_port: 0,
            bound: false,
            connected: false,
            listening: false,
            backlog: 0,
            shutdown_rd: false,
            shutdown_wr: false,
            rx_queue: VecDeque::new(),
            tcp_conn: None,
            in_use: false,
            ref_count: 0,
            pending_error: 0,
            unix_path: [0u8; 108],
            unix_path_len: 0,
        }
    }
}

pub const MAX_SOCKETS: usize = 16;

struct SocketTable {
    sockets: [Socket; MAX_SOCKETS],
    next_ephemeral_port: u16,
}

impl SocketTable {
    const fn new() -> Self {
        const INIT_SOCK: Socket = Socket::new();
        SocketTable {
            sockets: [INIT_SOCK; MAX_SOCKETS],
            next_ephemeral_port: 49152,
        }
    }
}

static SOCKETS: Spinlock<SocketTable> = Spinlock::new(SocketTable::new());

pub fn socket_create(domain: i32, sock_type: i32, protocol: i32) -> Option<usize> {
    // [IPv6 REAL] AF_INET6 (10) admitido para SOCK_STREAM (TCP de doble pila)
    if (domain != AF_INET && domain != 1 && domain != 10) || (sock_type != SOCK_DGRAM && sock_type != SOCK_STREAM && sock_type != 5) {
        return None;
    }
    let mut table = SOCKETS.lock();
    for i in 0..MAX_SOCKETS {
        if !table.sockets[i].in_use {
            table.sockets[i].in_use = true;
            table.sockets[i].ref_count = 1;
            table.sockets[i].domain = domain;
            table.sockets[i].sock_type = sock_type;
            table.sockets[i].protocol = protocol;
            table.sockets[i].local_port = table.next_ephemeral_port;
            table.next_ephemeral_port = table.next_ephemeral_port.wrapping_add(1);
            if table.next_ephemeral_port < 49152 {
                table.next_ephemeral_port = 49152;
            }
            table.sockets[i].bound = false;
            table.sockets[i].connected = false;
            table.sockets[i].listening = false;
            table.sockets[i].backlog = 0;
            table.sockets[i].shutdown_rd = false;
            table.sockets[i].shutdown_wr = false;
            table.sockets[i].rx_queue.clear();
            table.sockets[i].tcp_conn = None;
            return Some(i);
        }
    }
    None
}

pub fn socket_retain(id: usize) {
    if id < MAX_SOCKETS {
        let mut table = SOCKETS.lock();
        if table.sockets[id].in_use {
            table.sockets[id].ref_count = table.sockets[id].ref_count.saturating_add(1);
        }
    }
}

pub fn socket_close(id: usize) {
    if id < MAX_SOCKETS {
        let mut table = SOCKETS.lock();
        if table.sockets[id].in_use {
            if table.sockets[id].ref_count > 1 {
                table.sockets[id].ref_count -= 1;
            } else {
                table.sockets[id].ref_count = 0;
                table.sockets[id].in_use = false;
                table.sockets[id].unix_path_len = 0;
                // [CICLO 47-c] Ultima referencia: purgar conexiones AF_UNIX pendientes
                // (UNIX_PENDIENTES es un cerrojo distinto; se suelta SOCKETS antes).
                let era_unix = table.sockets[id].domain == 1;
                if era_unix { drop(table); unix_drop_pending(id); table = SOCKETS.lock(); }
                table.sockets[id].connected = false;
                table.sockets[id].listening = false;
                table.sockets[id].backlog = 0;
                table.sockets[id].bound = false;
                table.sockets[id].shutdown_rd = false;
                table.sockets[id].shutdown_wr = false;
                table.sockets[id].rx_queue.clear();
                if let Some(t_id) = table.sockets[id].tcp_conn.take() {
                    drop(table);
                    super::tcp::tcp_close(t_id);
                    return;
                }
            }
        }
    }
}

pub fn socket_bind(id: usize, port: u16) -> bool {
    if id >= MAX_SOCKETS { return false; }
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return false; }
    // Puerto 0 = "cualquiera": se conserva el efímero asignado al crear el
    // socket (antes quedaba 0 y los datagramas salían con puerto de origen 0;
    // slirp los respondía por IPv4 pero los descarta por IPv6).
    if port != 0 { table.sockets[id].local_port = port; }
    table.sockets[id].bound = true;
    true
}

pub fn socket_get_local(id: usize) -> Option<u16> {
    if id >= MAX_SOCKETS { return None; }
    let table = SOCKETS.lock();
    if !table.sockets[id].in_use { return None; }
    Some(table.sockets[id].local_port)
}

pub fn socket_get_peer_addr(id: usize) -> Option<(super::IpAddr, u16)> {
    if id >= MAX_SOCKETS { return None; }
    let table = SOCKETS.lock();
    if !table.sockets[id].in_use || !table.sockets[id].connected { return None; }
    Some((table.sockets[id].remote_ip, table.sockets[id].remote_port))
}

pub fn socket_get_peer(id: usize) -> Option<([u8; 4], u16)> {
    if id >= MAX_SOCKETS { return None; }
    let table = SOCKETS.lock();
    if !table.sockets[id].in_use { return None; }
    if !table.sockets[id].connected { return None; }
    Some((table.sockets[id].remote_ip.v4(), table.sockets[id].remote_port))
}

pub fn socket_shutdown(id: usize, how: i32) -> Result<(), i32> {
    if id >= MAX_SOCKETS { return Err(-9); } // -EBADF
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return Err(-9); }
    match how {
        0 => table.sockets[id].shutdown_rd = true, // SHUT_RD
        1 => table.sockets[id].shutdown_wr = true, // SHUT_WR
        2 => {
            table.sockets[id].shutdown_rd = true;
            table.sockets[id].shutdown_wr = true;
        } // SHUT_RDWR
        _ => return Err(-22), // -EINVAL
    }
    Ok(())
}

pub fn socket_connect(id: usize, ip: super::IpAddr, port: u16) -> Result<(), i32> {
    if id >= MAX_SOCKETS { return Err(-9); }
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return Err(-9); }
    let s = &mut table.sockets[id];
    s.remote_ip = ip;
    s.remote_port = port;
    s.connected = true;

    if s.sock_type == SOCK_STREAM {
        let local_port = s.local_port;
        drop(table);
        let res = super::tcp::tcp_connect(ip, port, local_port);
        let mut table = SOCKETS.lock();
        if id < MAX_SOCKETS && table.sockets[id].in_use {
            match res {
                Ok(tcp_id) => {
                    table.sockets[id].tcp_conn = Some(tcp_id);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            Err(-9)
        }
    } else {
        Ok(())
    }
}

/// [FRENTE B] Identificador de la conexión TCP asociada (None en UDP).
pub fn socket_tcp_conn(id: usize) -> Option<usize> {
    if id >= MAX_SOCKETS { return None; }
    let table = SOCKETS.lock();
    if !table.sockets[id].in_use { return None; }
    table.sockets[id].tcp_conn
}

pub fn socket_is_listening(id: usize) -> bool {
    if id >= MAX_SOCKETS { return false; }
    let table = SOCKETS.lock();
    table.sockets[id].in_use && table.sockets[id].listening
}

pub fn socket_listen(id: usize, backlog: i32) -> Result<(), i32> {
    if id >= MAX_SOCKETS { return Err(-9); }
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return Err(-9); }
    let s = &mut table.sockets[id];
    if s.sock_type == SOCK_STREAM {
        // [CIERRE RED] Escucha TCP real: conexión `Listen` en la tabla TCP.
        if s.tcp_conn.is_none() {
            let port = s.local_port;
            drop(table);
            let l = super::tcp::tcp_listen(port)?;
            let mut table = SOCKETS.lock();
            let s = &mut table.sockets[id];
            s.tcp_conn = Some(l); s.listening = true; s.backlog = if backlog > 0 { backlog } else { 128 };
            return Ok(());
        }
    }
    s.listening = true;
    s.backlog = if backlog > 0 { backlog } else { 128 };
    Ok(())
}

pub fn socket_accept(id: usize) -> Result<usize, i32> { socket_accept_impl(id, true) }

/// Variante no bloqueante (SOCK_NONBLOCK / O_NONBLOCK): -EAGAIN si no hay hija lista.
pub fn socket_accept_nowait(id: usize) -> Result<usize, i32> { socket_accept_impl(id, false) }

fn socket_accept_impl(id: usize, wait: bool) -> Result<usize, i32> {
    if id >= MAX_SOCKETS { return Err(-9); }
    let table = SOCKETS.lock();
    if !table.sockets[id].in_use { return Err(-9); }
    if !table.sockets[id].listening {
        return Err(-22); // -EINVAL
    }
    let domain = table.sockets[id].domain;
    let sock_type = table.sockets[id].sock_type;
    let protocol = table.sockets[id].protocol;
    let local_port = table.sockets[id].local_port;
    let listener = table.sockets[id].tcp_conn;
    drop(table);

    // [CIERRE RED] accept() REAL: espera una conexión hija ESTABLISHED del
    // `Listen` (3-way handshake completado por la pila TCP). Antes se
    // inventaba un par 127.0.0.1:5000x sin ninguna conexión detrás.
    let (tcp_id, rip, rport) = match listener {
        Some(l) if sock_type == SOCK_STREAM => match if wait { super::tcp::tcp_wait_accept(l, 1000) } else { super::tcp::tcp_accept(l) } { Some(v) => v, None => return Err(-11) },
        _ => return Err(-22),
    };

    let mut table = SOCKETS.lock();
    for i in 0..MAX_SOCKETS {
        if !table.sockets[i].in_use {
            let s = &mut table.sockets[i];
            s.in_use = true;
            s.ref_count = 1;
            s.domain = domain;
            s.sock_type = sock_type;
            s.protocol = protocol;
            s.local_port = local_port;
            s.remote_ip = rip;
            s.remote_port = rport;
            s.connected = true;
            s.listening = false;
            s.tcp_conn = Some(tcp_id);
            s.rx_queue.clear();
            return Ok(i);
        }
    }
    super::tcp::tcp_abort(tcp_id);
    Err(-24) // -EMFILE
}

pub fn socket_sendto(
    id: usize,
    payload: &[u8],
    dst_ip: super::IpAddr,
    dst_port: u16,
) -> Result<usize, &'static str> {
    if id >= MAX_SOCKETS { return Err("invalid socket"); }
    let src_port = {
        let table = SOCKETS.lock();
        if !table.sockets[id].in_use { return Err("socket not in use"); }
        table.sockets[id].local_port
    };

    match dst_ip {
        super::IpAddr::V4(d4) => send_udp(get_ip(), d4, src_port, dst_port, payload)?,
        super::IpAddr::V6(d6) => {
            // [IPv6 REAL] origen SLAAC/enlace según destino; exige RA previo
            if !d6.is_link_local() && !d6.is_multicast() && !super::ipv6::is_local_v6(&d6) && !super::ipv6::ensure_configured(100) { return Err("IPv6 sin autoconfigurar"); }
            let s6 = super::ipv6::pick_source(&d6);
            super::udp::send_udp6(&s6, &d6, src_port, dst_port, payload)?
        }
    }
    Ok(payload.len())
}

pub fn socket_recvfrom(
    id: usize,
    buf: &mut [u8],
) -> Option<(usize, super::IpAddr, u16)> {
    if id >= MAX_SOCKETS { return None; }
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return None; }

    if let Some(pkt) = table.sockets[id].rx_queue.pop_front() {
        let copy_len = buf.len().min(pkt.data.len());
        buf[..copy_len].copy_from_slice(&pkt.data[..copy_len]);
        Some((copy_len, pkt.src_ip, pkt.src_port))
    } else {
        None
    }
}

pub fn deliver_udp_to_socket(src_ip: super::IpAddr, src_port: u16, dst_port: u16, payload: &[u8]) -> bool {
    let mut table = SOCKETS.lock();
    let mut delivered = false;
    for i in 0..MAX_SOCKETS {
        if table.sockets[i].in_use && table.sockets[i].local_port == dst_port {
            if table.sockets[i].rx_queue.len() < 16 {
                let pkt = UdpPacket {
                    src_ip,
                    src_port,
                    data: payload.to_vec(),
                };
                table.sockets[i].rx_queue.push_back(pkt);
                delivered = true;
            }
        }
    }
    delivered
}

pub fn socket_write(id: usize, payload: &[u8]) -> Result<usize, i32> {
    if id >= MAX_SOCKETS { return Err(-9); }
    let (sock_type, tcp_conn, dst_ip, dst_port) = {
        let table = SOCKETS.lock();
        if !table.sockets[id].in_use { return Err(-9); }
        (table.sockets[id].sock_type, table.sockets[id].tcp_conn, table.sockets[id].remote_ip, table.sockets[id].remote_port)
    };

    if sock_type == SOCK_STREAM {
        if let Some(t_id) = tcp_conn {
            super::tcp::tcp_send(t_id, payload)
        } else {
            Err(-107) // -ENOTCONN
        }
    } else {
        match socket_sendto(id, payload, dst_ip, dst_port) {
            Ok(n) => Ok(n),
            Err(_) => Err(-5), // -EIO: la pila de red no pudo transmitir
        }
    }
}

pub fn socket_read(id: usize, buf: &mut [u8]) -> Result<usize, i32> {
    if id >= MAX_SOCKETS { return Err(-9); }
    let (sock_type, tcp_conn) = {
        let table = SOCKETS.lock();
        if !table.sockets[id].in_use { return Err(-9); }
        (table.sockets[id].sock_type, table.sockets[id].tcp_conn)
    };

    if sock_type == SOCK_STREAM {
        if let Some(t_id) = tcp_conn {
            super::tcp::tcp_recv(t_id, buf)
        } else {
            Err(-107) // -ENOTCONN
        }
    } else {
        match socket_recvfrom(id, buf) {
            Some((n, _, _)) => Ok(n),
            None => Err(-11), // -EAGAIN
        }
    }
}

pub fn socket_poll_status(id: usize) -> (bool, bool) {
    let table = SOCKETS.lock();
    if id < MAX_SOCKETS && table.sockets[id].in_use {
        let can_read = !table.sockets[id].rx_queue.is_empty();
        let can_write = true;
        (can_read, can_write)
    } else {
        (false, false)
    }
}


/// [IPv6] Anota un error asíncrono en el socket ligado a `local_port`.
pub fn socket_set_error_by_port(local_port: u16, err: i32) {
    let mut table = SOCKETS.lock();
    for i in 0..MAX_SOCKETS {
        if table.sockets[i].in_use && table.sockets[i].local_port == local_port && table.sockets[i].sock_type == SOCK_DGRAM {
            table.sockets[i].pending_error = err;
        }
    }
}

/// Devuelve y limpia el error pendiente del socket (0 si no hay).
pub fn socket_take_error(id: usize) -> i32 {
    if id >= MAX_SOCKETS { return 0; }
    let mut table = SOCKETS.lock();
    if !table.sockets[id].in_use { return 0; }
    let e = table.sockets[id].pending_error; table.sockets[id].pending_error = 0; e
}

/// ¿Es un socket de datagramas (UDP)?
pub fn socket_is_dgram(id: usize) -> bool {
    if id >= MAX_SOCKETS { return false; }
    let table = SOCKETS.lock();
    table.sockets[id].in_use && table.sockets[id].sock_type == SOCK_DGRAM
}


// ─────────────────────────────────────────────────────────────────────────────
//  [CICLO 47-c] Sockets AF_UNIX con nombre (sockaddr_un.sun_path)
//
//  Un socket AF_UNIX ligado a una ruta y en escucha acepta conexiones de otros
//  procesos del mismo nucleo. Cada conexion aceptada es un par de tuberias
//  (como `socketpair`): el cliente y el servidor reciben descriptores
//  `SocketPairEnd` cruzados. No hay red implicada: es IPC local real.
// ─────────────────────────────────────────────────────────────────────────────

/// Conexion pendiente de `accept`: (socket que escucha, tuberia a, tuberia b).
/// El cliente lee de `b` y escribe en `a`; el servidor lee de `a` y escribe en `b`.
static UNIX_PENDIENTES: Spinlock<Vec<(usize, u32, u32)>> = Spinlock::new(Vec::new());

pub fn socket_domain(id: usize) -> i32 {
    let table = SOCKETS.lock();
    if id < MAX_SOCKETS && table.sockets[id].in_use { table.sockets[id].domain } else { 0 }
}

/// bind(2) para AF_UNIX: EADDRINUSE si otra escucha ya usa la ruta.
pub fn unix_bind(id: usize, path: &[u8]) -> Result<(), i32> {
    if path.is_empty() || path.len() > 107 { return Err(-36); } // -ENAMETOOLONG
    let mut table = SOCKETS.lock();
    if id >= MAX_SOCKETS || !table.sockets[id].in_use { return Err(-9); }
    if table.sockets[id].domain != 1 { return Err(-22); }
    if table.sockets[id].unix_path_len != 0 { return Err(-22); } // ya ligado
    for i in 0..MAX_SOCKETS {
        let s = &table.sockets[i];
        if s.in_use && s.domain == 1 && s.unix_path_len as usize == path.len()
            && &s.unix_path[..path.len()] == path {
            return Err(-98); // -EADDRINUSE
        }
    }
    let s = &mut table.sockets[id];
    s.unix_path[..path.len()].copy_from_slice(path);
    s.unix_path_len = path.len() as u8;
    s.bound = true;
    Ok(())
}

pub fn unix_listen(id: usize, backlog: i32) -> Result<(), i32> {
    let mut table = SOCKETS.lock();
    if id >= MAX_SOCKETS || !table.sockets[id].in_use { return Err(-9); }
    if table.sockets[id].unix_path_len == 0 { return Err(-22); } // sin bind
    table.sockets[id].listening = true;
    table.sockets[id].backlog = if backlog <= 0 { 1 } else { backlog.min(128) };
    Ok(())
}

/// connect(2) para AF_UNIX: busca la ruta entre los sockets en escucha y deja
/// la conexion en la cola del servidor. Devuelve las tuberias del CLIENTE
/// (lectura, escritura).
pub fn unix_connect(path: &[u8]) -> Result<(u32, u32), i32> {
    let (listener, backlog) = {
        let table = SOCKETS.lock();
        let mut encontrado = None;
        for i in 0..MAX_SOCKETS {
            let s = &table.sockets[i];
            if s.in_use && s.domain == 1 && s.unix_path_len as usize == path.len()
                && &s.unix_path[..path.len()] == path {
                if !s.listening { return Err(-111); } // -ECONNREFUSED
                encontrado = Some((i, s.backlog));
                break;
            }
        }
        match encontrado { Some(x) => x, None => return Err(-2) } // -ENOENT
    };
    {
        let cola = UNIX_PENDIENTES.lock();
        if cola.iter().filter(|c| c.0 == listener).count() >= backlog.max(1) as usize {
            return Err(-11); // -EAGAIN: backlog lleno
        }
    }
    let a = crate::process::pipe_alloc().ok_or(-24)?;
    let b = match crate::process::pipe_alloc() {
        Some(x) => x,
        None => { crate::process::pipe_close_reader(a); crate::process::pipe_close_writer(a); return Err(-24); }
    };
    UNIX_PENDIENTES.lock().push((listener, a, b));
    Ok((b, a))
}

/// accept(2) para AF_UNIX: devuelve las tuberias del SERVIDOR (lectura, escritura).
pub fn unix_accept(id: usize, block: bool) -> Result<(u32, u32), i32> {
    let tope = crate::time::pit::ticks().wrapping_add(1000); // 10 s de seguridad
    loop {
        {
            let mut cola = UNIX_PENDIENTES.lock();
            if let Some(pos) = cola.iter().position(|c| c.0 == id) {
                let (_, a, b) = cola.remove(pos);
                return Ok((a, b));
            }
        }
        if !block { return Err(-11); }
        if crate::time::pit::ticks() >= tope { return Err(-11); }
        if crate::process::signals::current_sig_pending() != 0 { return Err(-4); } // -EINTR
        crate::process::scheduler::wait_hlt_preemptible();
    }
}

/// Elimina las conexiones pendientes de un socket que se cierra.
pub fn unix_drop_pending(id: usize) {
    let mut cola = UNIX_PENDIENTES.lock();
    cola.retain(|c| {
        if c.0 == id {
            crate::process::pipe_close_reader(c.1); crate::process::pipe_close_writer(c.1);
            crate::process::pipe_close_reader(c.2); crate::process::pipe_close_writer(c.2);
            false
        } else { true }
    });
}
