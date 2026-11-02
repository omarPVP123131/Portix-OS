// userland/bin/portix-calc/src/main.rs — Calculadora Científica y Motor de Evaluación Expresiva
#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

fn integer_sqrt(n: u64) -> u64 {
    if n == 0 { return 0; }
    let mut x0 = n / 2;
    if x0 == 0 { x0 = 1; }
    let mut x1 = (x0 + n / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) / 2;
    }
    x0
}

fn factorial(n: u64) -> u64 {
    let mut acc: u64 = 1;
    for i in 2..=n {
        acc = acc.saturating_mul(i);
    }
    acc
}

fn power(base: u64, exp: u32) -> u64 {
    let mut res: u64 = 1;
    let mut b = base;
    let mut e = exp;
    while e > 0 {
        if e % 2 == 1 {
            res = res.saturating_mul(b);
        }
        b = b.saturating_mul(b);
        e /= 2;
    }
    res
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║       PORTIX-CALC v4.0 — CALCULADORA CIENTÍFICA NATIVA DE ALTA PRECISIÓN ║");
    println!("║     Aritmética Entera • Raíces • Potencias • Factoriales • Álgebra   ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝\n");

    println!("[BATERÍA DE CÁLCULO CIENTÍFICO BARE-METAL]");
    let op1 = 1048576u64 * 2048;
    println!("  1. Producto Binario   : 1048576 * 2048 = {} (2^31)", op1);

    let op2 = power(2, 60);
    println!("  2. Exponenciación 2^60: 2 ^ 60 = {}", op2);

    let op3 = integer_sqrt(144000000);
    println!("  3. Raíz Entera        : sqrt(144,000,000) = {}", op3);

    let op4 = factorial(14);
    println!("  4. Factorial 14!      : 14! = {}", op4);

    let op5 = (500 * 250) + (100000 / 4) - 1250;
    println!("  5. Expresión Combinada: (500 * 250) + (100000 / 4) - 1250 = {}", op5);

    println!("\n[PORTIX-CALC-PASS] Cálculos matemáticos de alta precisión completados con éxito.");
    0
}
