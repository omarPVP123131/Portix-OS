# Estándares de Código y Reglas de Ingeniería (Clean Code) - Portix OS


> **Filosofía Fundamental**: Portix OS es un sistema operativo real, determinista y de alto rendimiento. En este repositorio no se tolera "código basura", soluciones de parche frágiles ni dependencias innecesarias. Todo archivo debe reflejar la calidad y disciplina de un Ingeniero de Sistemas Operativos Senior.

---

## 1. Principios Rectores de Arquitectura

### 1.1 Cero Dependencias Externas en el Kernel
- El archivo `kernel/Cargo.toml` debe mantener la sección `[dependencies]` estrictamente vacía.
- Solo se permite el uso de los crates centrales del compilador: `core` y `alloc`.
- Cualquier algoritmo, estructura de datos, protocolo o controlador debe ser implementado internamente con precisión algorítmica y matemáticamente verificable.

### 1.2 Prohibición de Pánicos No Controlados
- **Cero `.unwrap()` y Cero `.expect()` en espacio de kernel**: Todo flujo propenso a fallas debe propagar tipos monádicos `Option<T>` o `Result<T, KernelError>`.
- Si una operación crítica no puede continuar, se debe registrar el fallo detallado a través del subsistema de logging (`com1_print!`) y manejar una degradación controlada o código de error POSIX.

### 1.3 Desacoplamiento Estricto Kernel / Userland
- El kernel jamás debe asumir ni depender de estructuras de datos privadas de las aplicaciones de usuario.
- La frontera entre Ring-0 (Superusuario / Kernel) y Ring-3 (Espacio de Usuario) se rige exclusivamente por llamadas al sistema documentadas (`int $0x80`).
- Todo puntero proveniente del espacio de usuario debe validarse contra desbordamientos de buffer, punteros nulos y rangos de memoria antes de ser desreferenciado en el kernel.

---

## 2. Reglas Específicas por Lenguaje

### 2.1 Código en Rust (Kernel y Herramientas Base)
1. **Inmutabilidad por Defecto**: Las variables deben ser inmutables a menos que la mutación sea estrictamente necesaria (`mut` justificado y acotado).
2. **Uso de `unsafe` Cuarentenado**:
   - Todo bloque `unsafe` debe ir precedido de un comentario `// SAFETY: ...` explicando por qué es seguro, qué invariantes garantiza y cómo se previene la corrupción de memoria.
   - Minimizar la superficie de `unsafe`: envolver el código inseguro en abstracciones seguras tipo RAII.
3. **Manejo de Errores Idiomático**:
   - Preferir operadores de propagación `?` sobre declaraciones `match` anidadas excesivas.
4. **Cero Advertencias del Compilador**:
   - Ninguna compilación de Rust debe emitir warnings (`#[deny(warnings)]`). Todo código muerto, variable no usada o importación redundante debe eliminarse.

### 2.2 Código en C Estándar (Userland Libc y Aplicaciones)
1. **Conformidad con Estándares**:
   - El código en C debe apegarse a C99/C11 estándar, compilable con `-Wall -Wextra -Werror -pedantic -std=c99`.
2. **Gestión de Recursos y Fugas de Memoria**:
   - Todo `malloc()` debe tener un camino de ejecución que garantice su correspondiente `free()`.
   - Validar invariablemente el retorno de asignación (`if (ptr == NULL)`).
3. **Seguridad en Manejo de Cadenas**:
   - Queda terminantemente prohibido el uso de funciones inseguras como `gets()` o `strcpy()` sin control de longitud. Usar `strncpy`, `snprintf` y comprobación explícita de terminador nulo (`\0`).
4. **Modularidad e Inclusión**:
   - Los archivos de encabezado (`.h`) deben contar siempre con guardas de inclusión (`#ifndef FOO_H / #define FOO_H / #endif`).
   - Los tipos deben utilizar `stdint.h` (`uint32_t`, `size_t`, `int64_t`) en lugar de tipos con longitud indeterminada por plataforma (`long`, `short`).

### 2.3 Ensamblador (Bootloader y Rutinas de CPU)
1. **Comentarios de Invariantes de Registros**:
   - Toda función o rutina en ensamblador debe documentar qué registros modifica (clobbered registers) y qué parámetros espera.
2. **Alineación de Pila**:
   - En llamadas a código de 64 bits (AMD64 ABI), la pila `RSP` debe mantenerse estrictamente alineada a 16 bytes antes de emitir instrucciones `call`.

---

## 3. Prácticas de Clean Code y Legibilidad

### 3.1 Nombres Descriptivos y con Propósito
- **Variables**: Evitar nombres crípticos de una letra (excepto índices estándar como `i`, `j` en bucles cortos).
  - *Incorrecto*: `let x = get_b();`
  - *Correcto*: `let framebuffer_base = get_framebuffer_physical_address();`
- **Funciones**: Usar verbos que describan con exactitud la acción (`allocate_page`, `flush_tlb`, `parse_elf_header`).

### 3.2 Funciones Pequeñas y de Responsabilidad Única (SRP)
- Una función debe hacer una sola cosa y hacerla de manera impecable.
- Como norma general, si una función supera las 50-60 líneas, debe evaluarse su refactorización en subfunciones auxiliares puras.

### 3.3 Comentarios Significativos
- No comentar lo que el código ya dice de forma obvia.
- Comentar el **por qué** se toma una decisión técnica poco intuitiva, las restricciones del hardware de la CPU o los protocolos de bus (PCI, ATA, PIC).

---

## 4. Política de Control de Versiones y Calidad de Integración

1. **Compilación Limpia Obligatoria**:
   - Todo cambio debe pasar `python3 scripts/build.py --no-run` con código de retorno 0.
2. **Aprobación de la Suite CI**:
   - Antes de dar por finalizada una tarea, el validador headless (`python3 scripts/headless_runner.py`) debe ejecutarse y reportar el 100% de pruebas superadas sin fallas ni bloqueos.
3. **Registro Continuo en AGENTS.md**:
   - Cualquier modificación debe ser documentada con precisión en la bitácora técnica de `AGENTS.md`.
