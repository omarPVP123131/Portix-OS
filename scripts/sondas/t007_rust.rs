// Sonda Rust (fase posterior a C++): programa Rust corriente, compilado por el rustc del
// anfitrión para x86_64-unknown-linux-gnu con `-C target-feature=+crt-static` (std +
// glibc estática, sin modificar nada), que ejercita lo que std::thread pide al núcleo:
//   1. std::thread::spawn + Arc<Mutex>: 4 hilos x 20000 incrementos = 80000.
//   2. Condvar: 200 vueltas de ping-pong (FUTEX_WAIT/WAKE reales).
//   3. mpsc::channel: 3 productores x 1000 mensajes, un consumidor (suma exacta).
//   4. thread::scope con préstamo de la pila de main (hilos de alcance).
//   5. panic! dentro de un hilo: join() devuelve Err y el proceso sigue (unwinder).
//   6. Barrier de 4 hilos + RwLock con lectores concurrentes.
//   7. park/unpark y thread::sleep(300 ms) medido con Instant.
//   8. thread_local! con Drop por hilo, y nombres de hilo (prctl/PR_SET_NAME).
// Cada comprobación imprime "T007-n: ok" o "T007-n: FALLO …"; al final "[T007-PASS] …".
use std::sync::{mpsc, Arc, Barrier, Condvar, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use std::cell::RefCell;

static FALLOS: AtomicUsize = AtomicUsize::new(0);
fn resultado(n: u32, ok: bool, detalle: String) {
    println!("T007-{}: {} {}", n, if ok { "ok" } else { "FALLO" }, detalle);
    if !ok { FALLOS.fetch_add(1, Ordering::SeqCst); }
}

fn gettid() -> i64 { unsafe { libc_gettid() } }
extern "C" { #[link_name = "gettid"] fn libc_gettid() -> i64; }

static DROPS_TL: AtomicUsize = AtomicUsize::new(0);
struct Rastro(i64);
impl Drop for Rastro {
    fn drop(&mut self) { DROPS_TL.fetch_add(1, Ordering::SeqCst); println!("  [t007] Drop thread_local en tid={}", self.0); }
}
thread_local! { static RASTRO: RefCell<Option<Rastro>> = RefCell::new(None); }

fn main() {
    println!("[t007] Rust con hilos: pid={} tid={} paralelismo={}", std::process::id(), gettid(),
             thread::available_parallelism().map(|n| n.get()).unwrap_or(0));

    // 1. Arc<Mutex> con 4 hilos
    {
        let total = Arc::new(Mutex::new(0u64));
        let hs: Vec<_> = (0..4).map(|_| { let t = Arc::clone(&total); thread::spawn(move || { for _ in 0..20000 { *t.lock().unwrap() += 1; } }) }).collect();
        for h in hs { h.join().unwrap(); }
        let v = *total.lock().unwrap();
        resultado(1, v == 80000, format!("4 hilos x 20000 bajo Mutex = {}", v));
    }

    // 2. Condvar ping-pong
    {
        let par = Arc::new((Mutex::new(0u8), Condvar::new()));
        let p2 = Arc::clone(&par);
        let vueltas = Arc::new(AtomicUsize::new(0));
        let v2 = Arc::clone(&vueltas);
        let h = thread::spawn(move || {
            let (m, cv) = &*p2;
            for _ in 0..200 {
                let mut g = m.lock().unwrap();
                while *g != 1 { g = cv.wait(g).unwrap(); }
                v2.fetch_add(1, Ordering::SeqCst); *g = 0; cv.notify_one();
            }
        });
        let (m, cv) = &*par;
        for _ in 0..200 {
            let mut g = m.lock().unwrap();
            *g = 1; cv.notify_one();
            while *g != 0 { g = cv.wait(g).unwrap(); }
        }
        h.join().unwrap();
        let v = vueltas.load(Ordering::SeqCst);
        resultado(2, v == 200, format!("ping-pong con Condvar: {} vueltas", v));
    }

    // 3. mpsc
    {
        let (tx, rx) = mpsc::channel::<u64>();
        let hs: Vec<_> = (0..3).map(|p| { let tx = tx.clone(); thread::spawn(move || { for k in 0..1000u64 { tx.send(p * 1000 + k).unwrap(); } }) }).collect();
        drop(tx);
        let (mut n, mut suma) = (0u64, 0u64);
        for v in rx { n += 1; suma += v; }
        for h in hs { h.join().unwrap(); }
        resultado(3, n == 3000 && suma == (0..3000u64).sum::<u64>(), format!("mpsc: {} mensajes, suma {}", n, suma));
    }

    // 4. thread::scope
    {
        let datos: Vec<u64> = (1..=1000).collect();
        let mut parciales = [0u64; 4];
        thread::scope(|s| {
            for (slot, trozo) in parciales.iter_mut().zip(datos.chunks(250)) {
                s.spawn(move || { *slot = trozo.iter().sum(); });
            }
        });
        let total: u64 = parciales.iter().sum();
        resultado(4, total == 500500, format!("thread::scope sobre datos prestados de main: suma {}", total));
    }

    // 5. panic en hilo
    {
        let h = thread::Builder::new().name("hilo-que-cae".into()).spawn(|| { if gettid() > 0 { panic!("caida controlada"); } 1 }).unwrap();
        let r = h.join();
        let ok = match &r { Err(e) => e.downcast_ref::<&str>().map_or(false, |s| *s == "caida controlada"), Ok(_) => false };
        resultado(5, ok, "panic! en un hilo: join() devuelve Err con el mensaje y main sigue vivo".into());
    }

    // 6. Barrier + RwLock
    {
        let barrera = Arc::new(Barrier::new(4));
        let lock = Arc::new(RwLock::new(vec![1u32, 2, 3]));
        let lideres = Arc::new(AtomicUsize::new(0));
        let hs: Vec<_> = (0..4).map(|i| {
            let (b, l, ld) = (Arc::clone(&barrera), Arc::clone(&lock), Arc::clone(&lideres));
            thread::spawn(move || {
                if i == 0 { l.write().unwrap().push(4); }
                if b.wait().is_leader() { ld.fetch_add(1, Ordering::SeqCst); }
                l.read().unwrap().iter().sum::<u32>()
            })
        }).collect();
        let sumas: Vec<u32> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        let ok = lideres.load(Ordering::SeqCst) == 1 && sumas.iter().all(|&s| s == 10);
        resultado(6, ok, format!("Barrier de 4 (1 lider) y RwLock: lecturas {:?}", sumas));
    }

    // 7. park/unpark + sleep medido
    {
        let t0 = Instant::now();
        let h = thread::spawn(|| { thread::park(); thread::sleep(Duration::from_millis(300)); });
        thread::sleep(Duration::from_millis(50));
        h.thread().unpark();
        h.join().unwrap();
        let ms = t0.elapsed().as_millis();
        resultado(7, (340..3000).contains(&ms), format!("park/unpark + sleep(300 ms) en un hilo: {} ms", ms));
    }

    // 8. thread_local con Drop y nombres
    {
        let hs: Vec<_> = (0..3).map(|i| thread::Builder::new().name(format!("obrero-{}", i)).spawn(move || {
            RASTRO.with(|r| *r.borrow_mut() = Some(Rastro(gettid())));
            (thread::current().name().map(|s| s.to_string()), gettid())
        }).unwrap()).collect();
        let r: Vec<(Option<String>, i64)> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        let tids: Vec<i64> = r.iter().map(|x| x.1).collect();
        let nombres_ok = r.iter().enumerate().all(|(i, x)| x.0.as_deref() == Some(&format!("obrero-{}", i)));
        let distintos = tids[0] != tids[1] && tids[1] != tids[2] && tids[0] != tids[2] && !tids.contains(&gettid());
        let drops = DROPS_TL.load(Ordering::SeqCst);
        resultado(8, nombres_ok && distintos && drops == 3, format!("thread_local con Drop ({} drops) y nombres de hilo; tids {:?}", drops, tids));
    }

    let f = FALLOS.load(Ordering::SeqCst);
    if f == 0 { println!("[T007-PASS] Rust std con hilos: spawn, Mutex, Condvar, mpsc, scope, panic+join, Barrier/RwLock, park, thread_local"); }
    else { println!("[T007-FAIL] {} comprobaciones fallaron", f); std::process::exit(1); }
}
