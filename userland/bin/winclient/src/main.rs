#![no_std]
#![no_main]

use portix_rt::{println, getpid, mkdir, File, O_CREAT, O_WRONLY, O_TRUNC};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let pid = getpid();
    println!("============================================================");
    println!(" [WINCLIENT] Aplicacion Cliente de Ventana en Ring 3        ");
    println!(" [WINCLIENT] PID: {} | Protocolo IPC Desacoplado            ", pid);
    println!("============================================================");

    let _ = mkdir("/tmp/wm", 0o755);
    match File::open("/tmp/wm/client.win", O_CREAT | O_WRONLY | O_TRUNC) {
        Ok(f) => {
            let msg = b"TITLE=Cliente Ring 3;PID=";
            let _ = f.write(msg);
            println!("[WINCLIENT] Ventana registrada exitosamente en /tmp/wm/client.win");
            println!("[WINCLIENT] Conectado al Servidor de Ventanas (portix-wm)");
        }
        Err(e) => {
            println!("[WINCLIENT] Error al registrar ventana: {}", e);
            return 1;
        }
    }

    println!("[WINCLIENT] Sesion de cliente activa. Ejecucion completada.");
    0
}
