# Contribuir a Portix OS

¡Qué bueno que quieras ayudar en este viaje al bajo nivel! Portix es un proyecto exigente pero gratificante. Aquí tienes lo necesario para empezar.

## Requisitos Previos

Para compilar el kernel, necesitarás las siguientes herramientas en tu sistema:

1. Rust Nightly: Usamos características inestables de Rust.
   * rustup toolchain install nightly
   * rustup component add rust-src --toolchain nightly

2. Ensamblador NASM: Para los archivos de arranque en boot/ e ISRs.

3. QEMU: Para emular el sistema (qemu-system-x86_64).

4. LLVM Binutils: Para operaciones de enlazado y manejo de binarios.

## Cómo Empezar

1. Fork y Clonación: Haz un fork del repositorio y clónalo localmente.

2. Configuración: Asegúrate de tener instalado el target de bare-metal si es necesario (aunque usamos archivos .json de target personalizados).

3. Compilación: Utiliza los scripts en la carpeta /scripts. Este comando compila todo el proyecto:
   * python scripts/build.py


   
## Reglas del Código

* No Estándar (no_std): Todo el código debe funcionar sin la librería estándar de Rust.

* Unsafe: Minimiza el uso de unsafe. Si es necesario (que en un kernel lo es), documenta por qué es seguro o por qué es la única forma de acceder al hardware.

* Formato: Mantén el código limpio y legible siguiendo la estructura actual del proyecto.

## Estructura del Mensaje

Cada commit debe seguir este formato:

tipo(alcance): descripción breve en minúsculas

## Tipos de Commit

* feat: Una nueva característica (ej. un nuevo driver o comando).
* fix: Corrección de un error (ej. un kernel panic o un bug en el allocator).
* docs: Cambios solo en la documentación.
* style: Cambios que no afectan el significado del código (formateo, espacios, etc).
* refactor: Cambio en el código que ni corrige un error ni añade una función.
* arch: Cambios específicos en el arranque o ensamblador (boot/, ISRs).

## Ejemplos

* feat(drivers): añadir soporte inicial para ratón ps/2
* fix(mem): corregir desbordamiento en el buddy allocator
* docs(readme): actualizar guía de ejecución con qemu
* arch(boot): migrar stage2 a modo largo 64-bit
* feat(cmd): añadir comando 'disk' para listar particiones

## Reglas de Commits

1. Mensajes en minúsculas: Ayuda a la legibilidad rápida.
2. Sé descriptivo pero breve: El título no debe pasar los 50 caracteres.
3. Commits atómicos: Intenta que cada commit haga una sola cosa. Si arreglas un bug y añades una función, hazlo en dos commits separados.
4. 
## Áreas de Interés

Si no sabes por dónde empezar, estas áreas siempre necesitan amor:

* Nuevos comandos para la terminal en kernel/src/console/terminal/commands/.
* Optimización del renderizado de fuentes en kernel/src/graphics/render/.
* Mejoras en el manejo de interrupciones o soporte ACPI.

## Comunicación

Si tienes dudas técnicas, abre un Issue con el tag question o contacta con Omar Palomares. ¡Toda ayuda es valiosa!