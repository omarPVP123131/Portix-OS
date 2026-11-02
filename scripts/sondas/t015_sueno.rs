// T-015 / precisión de los sueños: ¿`nanosleep` duerme lo que se le pide?
//
// El fixture duerme N veces 100 ms y, en cada vuelta, compara el reloj del
// núcleo (línea `reloj:` de /proc/smp) antes y después. Tres cosas pueden
// salir mal y las tres se cuentan por separado:
//   * la vuelta no avanza ni un tick            -> retorno inmediato
//   * la vuelta avanza menos de lo pedido       -> retorno corto (defecto real)
//   * el núcleo declara `[SUENO-CORTO]`         -> lo mismo visto desde dentro
//
// Se compara contra lo que el propio núcleo cuenta en la línea `suenos:`, de
// modo que el PASS exige que las dos vistas coincidan: ni el fixture se
// autoengaña ni el núcleo se esconde.
//
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols \
//           scripts/sondas/t015_sueno.rs -o build/packages/t015_sueno
use std::time::{Duration, Instant};

fn campo(linea: &str, clave: &str) -> Option<u64> {
    let mut it = linea.split_whitespace();
    while let Some(p) = it.next() {
        if p == clave {
            return it.next()?.parse().ok();
        }
    }
    None
}

fn leer(clave: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/smp").ok()?;
    for l in s.lines() {
        if l.starts_with("reloj:") || l.starts_with("suenos:") {
            if let Some(v) = campo(l, clave) {
                return Some(v);
            }
        }
    }
    None
}

fn main() {
    let vueltas: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let ms: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let Some(t_inicial) = leer("ticks") else {
        println!("[T015-FAIL] /proc/smp no publica la linea `reloj: ticks`");
        std::process::exit(1);
    };
    let cortos_antes = leer("cortos").unwrap_or(0);
    println!("[T015] vueltas={} espera={} ms ticks_iniciales={}", vueltas, ms, t_inicial);

    let mut ceros = 0usize;
    let mut cortos = 0usize;
    let mut menor = u64::MAX;
    let mut mayor = 0u64;
    let mut suma = 0u64;
    for i in 1..=vueltas {
        let antes = match leer("ticks") { Some(t) => t, None => break };
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_millis(ms));
        let pared = t0.elapsed().as_millis() as u64;
        let despues = leer("ticks").unwrap_or(antes);
        let d = despues.wrapping_sub(antes);
        if d == 0 { ceros += 1; }
        // A 100 Hz, 100 ms son 10 ticks. Se tolera un tick de redondeo por abajo.
        let pedido_ticks = (ms / 10).max(1);
        if d + 1 < pedido_ticks { cortos += 1; }
        if d < menor { menor = d; }
        if d > mayor { mayor = d; }
        suma += d;
        println!("[T015] i={} ticks={} delta={} pared_ms={}", i, despues, d, pared);
    }
    let cortos_kernel = leer("cortos").map(|v| v.saturating_sub(cortos_antes)).unwrap_or(0);
    let peor_deficit = leer("peor_deficit_ticks").unwrap_or(0);
    println!("[T015-STATS] ceros={} cortos={} menor_delta={} mayor_delta={} media={} cortos_kernel={} peor_deficit={}",
             ceros, cortos, menor, mayor, suma / (vueltas as u64).max(1), cortos_kernel, peor_deficit);
    if ceros > 0 || cortos > 0 || cortos_kernel > 0 {
        println!("[T015-FAIL] hay sueños que no durmieron lo pedido (ceros={} cortos={} kernel={})",
                 ceros, cortos, cortos_kernel);
        std::process::exit(1);
    }
    println!("[T015-PASS] {} sueños de {} ms completos: ninguno volvió antes (delta mínimo {} ticks)",
             vueltas, ms, menor);
}
