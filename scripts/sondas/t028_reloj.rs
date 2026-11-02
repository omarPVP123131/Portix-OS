// T-028 / T-015 / T-033: ¿corre el reloj cuando el sistema está ocioso?
//
// La traza original de T-028 dejó dos relojes independientes que NO vencieron:
// el `sleep_until` del `curl` (más de 144 s con un plazo de 1,5 s) y el tope de
// 1000 ticks de `tcp_wait_accept`. Que dos plazos distintos se queden cortos a
// la vez sólo se explica si lo que se paró fue el reloj (o el barrido que
// despierta a los que duermen), no la syscall de turno.
//
// Este fixture mide eso desde fuera, sin creerle a nadie:
//   * `base`  — mide la frecuencia del reloj (ticks por segundo).
//   * `reloj` — deja dos hijos durmiendo (procesos Blocked con plazo, que es el
//               estado del `curl` de T-028) y el padre imprime `ticks` cada
//               100 ms. El ANFITRIÓN pone la hora: si el número de `ticks` se
//               repite durante segundos de pared, el reloj está parado; si las
//               líneas dejan de llegar, el que se colgó es el que duerme.
//
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols \
//           scripts/sondas/t028_reloj.rs -o build/packages/t028_reloj
use std::time::Duration;

fn ticks() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/smp").ok()?;
    for l in s.lines() {
        if let Some(resto) = l.strip_prefix("reloj: ticks ") {
            return resto.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn main() {
    let modo = std::env::args().nth(1).unwrap_or_else(|| String::from("base"));
    let Some(t0) = ticks() else {
        println!("[T028-FAIL] /proc/smp no publica la linea `reloj: ticks` (instrumentacion ausente)");
        std::process::exit(1);
    };
    match modo.as_str() {
        "base" => {
            std::thread::sleep(Duration::from_millis(1000));
            let t1 = ticks().unwrap_or(t0);
            let d = t1.wrapping_sub(t0);
            println!("[T028-BASE] ticks por segundo = {} (t0={} t1={})", d, t0, t1);
            if d == 0 {
                println!("[T028-FAIL] un segundo de pared sin un solo tick: el reloj no corre");
                std::process::exit(1);
            }
            println!("[T028-BASE-PASS] el reloj avanza con el sistema recien arrancado");
        }
        "reloj" => {
            let n: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(30);
            // Dos hijos durmiendo 60 s: Blocked con plazo, como el `curl` de T-028.
            let mut hijos = Vec::new();
            for _ in 0..2 {
                hijos.push(std::process::Command::new("/bin/sleep").arg("60").spawn());
            }
            let vivos = hijos.iter().filter(|h| h.is_ok()).count();
            println!("[T028-RELOJ] hijos durmiendo={} iteraciones={}", vivos, n);
            let mut anterior = ticks().unwrap_or(0);
            let mut repeticiones = 0usize;
            let mut peor = 0u64;
            for i in 1..=n {
                let t = ticks().unwrap_or(anterior);
                let d = t.wrapping_sub(anterior);
                if d == 0 { repeticiones += 1; } else { repeticiones = 0; }
                if d > peor { peor = d; }
                println!("[T028-RELOJ] i={} ticks={} delta={} sin_avanzar={}", i, t, d, repeticiones);
                // Tres iteraciones seguidas (300 ms de plazo cada una) sin que el
                // contador se mueva: el reloj no acompaña a las esperas.
                if repeticiones >= 3 {
                    println!("[T028-FAIL] el contador de ticks lleva {} lecturas sin avanzar mientras el padre duerme", repeticiones);
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(100));
                anterior = t;
            }
            println!("[T028-RELOJ-STATS] iteraciones={} peor_delta_ticks={} (una espera de 100 ms no deberia saltarse varias)", n, peor);
            println!("[T028-RELOJ-PASS] el reloj sigue avanzando con todos los demas procesos durmiendo");
        }
        otro => {
            println!("[T028-FAIL] modo desconocido: {}", otro);
            std::process::exit(2);
        }
    }
}
