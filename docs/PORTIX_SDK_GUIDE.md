# 🛠️ Guía Oficial del SDK para Desarrolladores — Portix OS v4.0 'Cosmos'

> **Fecha de Publicación**: Septiembre de 2026  
> **Versión del SDK**: Portix SDK v4.0  
> **Lenguaje de Desarrollo**: Rust (`nightly`, `#![no_std]`)  
> **Entorno de Ejecución**: Ring 3 (Espacio de Usuario con Aislamiento y Fast-IPC)  

---

## 📑 Índice de Contenidos

1. [Introducción y Filosofía del SDK](#1-introducción-y-filosofía-del-sdk)
2. [Estructura de un Proyecto Nativo](#2-estructura-de-un-proyecto-nativo)
3. [Manifiesto de Aplicación (`portix-app.json`)](#3-manifiesto-de-aplicación-portix-appjson)
4. [Catálogo de APIs de `portix-rt`](#4-catálogo-de-apis-de-portix-rt)
   - [E/S y Consola (`portix_rt::io`)](#es-y-consola-portix_rtio)
   - [Sistema de Archivos VFS (`portix_rt::fs`)](#sistema-de-archivos-vfs-portix_rtfs)
   - [Comunicación entre Procesos Fast-IPC (`portix_rt::ipc`)](#comunicación-entre-procesos-fast-ipc-portix_rtipc)
   - [Redes y Criptografía TLS 1.3 (`portix_rt::net`)](#redes-y-criptografía-tls-13-portix_rtnet)
   - [Audio y Multimedia (`portix_rt::audio`)](#audio-y-multimedia-portix_rtaudio)
   - [Inteligencia Artificial y Aceleración NPU (`portix_rt::ai`)](#inteligencia-artificial-y-aceleración-npu-portix_rtai)
5. [Tutorial Práctico: Creación de una App en 5 Minutos](#5-tutorial-práctico-creación-de-una-app-en-5-minutos)
6. [Empaquetado y Distribución en la App Store P2P](#6-empaquetado-y-distribución-en-la-app-store-p2p)

---

## 1. Introducción y Filosofía del SDK

El **SDK de Portix OS** proporciona a los desarrolladores un conjunto de herramientas de alto rendimiento para crear aplicaciones nativas ultraligeras, seguras y deterministas en Rust:

- **Cero Sobrecarga de Runtime**: Las aplicaciones enlazan directamente con la biblioteca en tiempo de ejecución `portix-rt`, sin dependencias pesadas de libc ni recolectores de basura.
- **Seguridad Garantizada por Rust**: Todo el código de aplicación se beneficia de la comprobación estricta de préstamos (*borrow checker*), garantizando la ausencia de corrupciones de memoria y carreras de datos.
- **Fast-IPC Híbrido**: Mensajería sincrónica en registros de CPU (latencia de 24 ns) y objetos de memoria compartida (VMOs) para grandes transferencias de datos sin copias intermedias.
- **Doble Compatibilidad**: Las aplicaciones pueden ejecutarse como binarios ELF64 estándar sobre la ABI de Linux o aprovechar las extensiones nativas soberanas de Portix OS.

---

## 2. Estructura de un Proyecto Nativo

Para inicializar una nueva aplicación nativa, utiliza la herramienta CLI del SDK:

```bash
python3 scripts/portix_sdk.py new mi_aplicacion
```

Esto generará la siguiente estructura de archivos:

```text
mi_aplicacion/
├── Cargo.toml            # Definición del paquete y dependencias de portix-rt
├── portix-app.json       # Manifiesto y permisos para la App Store
└── src/
    └── main.rs           # Punto de entrada de la aplicación (#![no_std])
```

---

## 3. Manifiesto de Aplicación (`portix-app.json`)

El archivo `portix-app.json` define los metadatos requeridos por el gestor de paquetes y el compositor Wayland:

```json
{
  "name": "mi_aplicacion",
  "version": "1.0.0",
  "description": "Monitor de rendimiento en tiempo real",
  "category": "Sistema",
  "author": "Desarrollador Soberano",
  "license": "GPL-3.0",
  "capabilities": [
    "CAP_FS_READ",
    "CAP_NET_CLIENT"
  ],
  "target_arch": [
    "x86_64",
    "aarch64"
  ]
}
```

---

## 4. Catálogo de APIs de `portix-rt`

### E/S y Consola (`portix_rt::io`)
Permite imprimir texto formateado en consola, leer entradas de teclado e interactuar con secuencias de escape TrueColor ANSI:

```rust
use portix_rt::{print, println};

println!("Iniciando procesamiento...");
```

### Sistema de Archivos VFS (`portix_rt::fs`)
Operaciones transaccionales sobre almacenamiento NVMe / FAT32:

```rust
use portix_rt::fs;

// Escribir un búfer en disco
let datos = b"Configuracion guardada exitosamente";
let _ = fs::write("/etc/mi_app.conf", datos);

// Leer un archivo completo
if let Ok(contenido) = fs::read("/etc/mi_app.conf") {
    // Procesar contenido...
}
```

### Comunicación entre Procesos Fast-IPC (`portix_rt::ipc`)
Intercambio sincrónico de mensajes con canales de tipo seL4:

```rust
use portix_rt::ipc::{Channel, Message};

let canal = Channel::connect("portix.wm.compositor").expect("Conexión fallida");
let msg = Message::new(0x01, &[/* datos de superficie */]);
let respuesta = canal.call(&msg);
```

### Redes y Criptografía TLS 1.3 (`portix_rt::net`)
Conexiones seguras punto a punto con cifrado ChaCha20-Poly1305:

```rust
use portix_rt::tls::Tls13Client;

let mut tls = Tls13Client::connect("api.portix.org", [10, 0, 2, 2], 443).unwrap();
let _ = tls.write_encrypted(b"GET /status HTTP/1.1\r\nHost: api.portix.org\r\n\r\n");
```

### Audio y Multimedia (`portix_rt::audio`)
Reproducción y captura de audio a través de la controladora AC97 / Intel HDA:

```rust
use portix_rt::audio::{PcmStream, SampleRate};

let mut stream = PcmStream::open(SampleRate::Hz44100).unwrap();
stream.write_samples(&búfer_pcm);
```

### Inteligencia Artificial y Aceleración NPU (`portix_rt::ai`)
Inferencia on-device local sin conexión a servidores externos:

```rust
use portix_rt::ai::NpuEngine;

let mut npu = NpuEngine::init().expect("NPU no disponible");
let prediccion = npu.evaluate_tensor(&tensor_entrada);
```

---

## 5. Tutorial Práctico: Creación de una App en 5 Minutos

A continuación se muestra un ejemplo completo de una utilidad de diagnóstico de memoria:

```rust
#![no_std]
#![no_main]

extern crate alloc;
use portix_rt::{println, syscall};

#[no_mangle]
pub extern "C" fn main(_argc: isize, _argv: *const *const u8) -> isize {
    println!("=== Portix MemChecker v1.0 ===");

    // Consultar estadísticas de memoria física vía syscall
    let mut stats = [0u64; 4];
    let ret = syscall::sys_sysinfo(&mut stats as *mut _ as *mut u8);

    if ret == 0 {
        let total_mb = stats[0] / (1024 * 1024);
        let libre_mb = stats[1] / (1024 * 1024);
        println!("Memoria Física Total : {} MB", total_mb);
        println!("Memoria Disponible   : {} MB", libre_mb);
        println!("Uso de RAM en Reposo : {} MB", total_mb.saturating_sub(libre_mb));
    }

    0
}
```

---

## 6. Empaquetado y Distribución en la App Store P2P

Una vez finalizada la aplicación, genera el paquete distribuible con firma de integridad:

```bash
python3 scripts/portix_sdk.py package mi_aplicacion
```

El comando generará el archivo `mi_aplicacion.pkg` en `build/dist/`, listo para ser publicado en la red de malla P2P de Portix OS o integrado directamente en la imagen de arranque.

---

<div align="center">

**Portix OS v4.0 'Cosmos' SDK**  
*Desarrolla el futuro del software nativo.*

</div>
