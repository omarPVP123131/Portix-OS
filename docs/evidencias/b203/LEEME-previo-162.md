# Evidencias de b203

Este directorio conserva registros **de texto**, no imágenes. Se guardan aquí porque `build/` y `/tmp` no están garantizados en las instantáneas del entorno. `SHA256SUMS` permite detectar cambios en los archivos enumerados; no autentica su origen.

| Pasada | Resultado registrado | Observaciones |
|---|---|---|
| 1 | 3810/3812 | Dos supuestos del ejecutor sobre el orden de instalación y el contador acumulado. |
| 2 | 3812/3813 | Falló HTTPS/IPv6 local; T-028 sigue abierto. |
| 3 | 3813/3813 | Primera pasada verde, después interrumpida por la 4. |
| 4 | 3811/3813 | Dos límites de T-026 incumplidos. La causa no está demostrada. |
| 5 | 3813/3813 | Verde; e2fsck, T-001 y L2b también pasaron. |

`recert128.log` recoge la recertificación que reconstruyó imágenes después de la pasada 5. La pasada 6 se inició después de esa reconstrucción; falta probar que la imagen sea idéntica a la usada por la 5. Los mapas de símbolos guardados coinciden al normalizar los sufijos LLVM, pero eso no prueba igualdad binaria. `suite113f-huella-parcial.txt` registra la huella del núcleo y del ejecutor durante la pasada 6 y los metadatos de la imagen base, **no** el SHA-256 de la imagen entera.

Los registros `suite*.log.gz` son la salida del ejecutor; `serie_suite*.log.gz` es el registro serie del invitado. El analizador acepta registros comprimidos o descomprimidos:

```sh
python3 scripts/auditar_evidencia_b203.py docs/evidencias/b203/serie_suite113e.log.gz
python3 scripts/test_auditar_evidencia_b203.py
```

La auditoría textual comprueba que `pkg` **informó** que el tamaño y SHA-256 eran correctos y que instaló el paquete; no calcula otra vez el SHA-256 del `.deb`. El entorno se reinició durante la suite 6 y se perdió su resultado final; sólo se conserva el registro parcial. La suite 6 no se cuenta como aprobada.

## Sondas posteriores: no son suites ni recertificación

| Registro | Resultado y alcance |
|---|---|
| `build114.log`, `build114-huellas.sha256` | Construcción completa anterior; las pruebas de BIOS y UEFI parciales no cierran b203. |
| `sonda-java-reuso114b.log` | Java versión fuera de plazo; clase 8/8 pero el PID líder salió 137. Sonda global fallida. |
| `t034b-anfitrion.log`, `t034-base114b.log`, `t034-k115.log` | Mismo fixture congelado `t034b-fuente.rs`: Linux 5/5, build 114 1/5 y núcleo experimental 115 5/5. |
| `t034d-anfitrion.log`, `t034-base114d.log`, `t034-k115d.log`, `t034-k116.log` | Fixture congelado `t034d-fuente.rs`: Linux 7/7; build 114 1/7; núcleo 115 5/7 porque volvió a ejecutar usuario después de `exit(2)`; núcleo 116 7/7. |
| `sonda-java-reuso116.log` | En copia BIOS con núcleo 116, clase 8/8 y código 0 en 161,0 s; versión 0 en 46,4 s; mmap y T-026 pasaron. Es anterior a la corrección adicional de señales. |
| `t034-k116f.log` | SIGKILL externo a líder dormido con hilos vivos no terminó en 50 s; no fue PASS. |
| `t034-k117.log`, `t034-k117g-solo.log` | Dos fallos conservados de la sonda: afinidad del hijo no garantizada y un sufijo incompleto en el predicado. La traza del segundo sí contiene señal 9 correcta, pero su resultado original sigue siendo FAIL. |
| `t034-k117g.log`, `t034-k117h-solo.log` | Tras corregir sonda y predicado: 8/8 en QEMU y 2/2 adicionales de `waitpid` de hijos, respectivamente. |
| `t034h-anfitrion.log`, `t034h-controles-locales.log`, `t034-k118h.log` | Fixture ampliado: 10/10 en Linux, 13/13 controles de predicados y 10/10 en QEMU con núcleo experimental 118. |
| `sonda-java-reuso118.log` | No hubo Java: el núcleo no detectó ATA y arrancó sin raíz/PID 1. FAIL conservado. |
| `sonda-java-reuso118b.log` | Reintento único: clase 8/8, código 0 y 78,5 s; versión superó 120 s y no acabó en la ventana adicional de 20 s. FAIL e incompleta; contador final no medido. |
| `controles-finales-b203.log` | 7/7 pruebas puras de estado de señales y 27/27 controles de predicados; comprobación de hashes. No sustituye ejecución QEMU. |
| `t032-base118-{01,02,03,04}.log` y resumen | 4/4 arranques ATA/raíz/init en copia BIOS de kernel 118; no se reprodujo el fallo previo. |
| `t032-traza118-*`, `t032-traza2*-*` | 1 + 3 arranques correctos con traza IDE de QEMU; los tres últimos también capturan estado alternativo y pasan el control de cobertura. Las trazas completas están comprimidas sin pérdida. No demuestran causa del fallo. |
| `t033-version-aislada118.log` | `java -version` en arranque limpio: FAIL a 120 s, salida real 0 dentro de los 20 s diagnósticos; delta 1 687 638 shootdowns; 6077 páginas desde fichero; 219296 µs y 171 IRQ perdidas. |
| `t033-tlb-anfitrion.log`, `t033-tlb-k118.log` | Reserva y liberación de 4096 páginas: 0/0 shootdowns sin hilo remoto, 4104/4100 con hilo en CPU 1. Semántica de la sonda: Linux 2/2 y QEMU 2/2. No se probaron todas las carreras SMP. |
| `t032-t033-controles-finales.log`, `t033-tlb-controles.log` | 82/82 controles de anfitrión, incluidos 13 negativos/positivos del predicado TLB; no sustituyen una suite. |

Los archivos `kernel11*-t034.sha256` y `imagen11*-*.sha256` identifican las fuentes y las imágenes usadas mientras existan en `build/`. Las imágenes se hicieron mediante inyección del núcleo en **copias BIOS**; `/boot` y UEFI no se actualizaron. Ninguna es una imagen dual final. El código y los predicados de versiones anteriores están congelados como `t034*-fuente.rs` y `t034*-predicados.py`. El primer ensayo `t034c-anfitrion.log` abortó por intentar desenrollar `pthread_exit` a través de Rust; se reemplazó por la syscall Linux `exit(2)`.

`T-032`: `t032-traza118-secuencia.log` conserva el extracto legible, y `t032-traza2-118-analisis.log` los conteos y SHA-256 de las trazas originales descomprimidas. Los `.gz` se validaron comparando el hash de los bytes descomprimidos con el original. Son trazas de arranques que pasaron y el trazado puede alterar tiempos. La sonda ATA exige una línea real de disco primario, ext2 e init, y no acepta modo degradado; también comprueba el SHA-256 de la imagen de entrada antes y después.

`T-033`: `t033-tlb-fuente.rs` y `t033-tlb-predicados.py` congelan la prueba usada con kernel 118. El binario estático se recompila con el comando indicado en la fuente; el hash de la fuente, binario e imagen está en `t032-t033-huellas-finales.txt`. El fixture se sirvió desde el anfitrión por HTTP a la copia privada de la VM y se comprobó su tamaño en el invitado; **no se verificó su SHA-256 dentro del invitado**. Las dos versiones previas de la fuente/ejecución de anfitrión se conservan. El ejecutor de la prueba de Java y el de ATA quedaron congelados antes de sus ajustes posteriores; estos ajustes se verificaron con controles locales, no con una nueva VM. La comprobación T-026 de la sonda Java se reforzó después para exigir ambos límites, con control local sobre la serie real. No hubo parche de kernel en esta ronda.

El manifiesto anterior de 112 archivos se conserva como `SHA256SUMS-previo-112.txt`, junto a su `LEEME-previo-112.md`; se verificó íntegro sustituyendo sólo el nombre del antiguo LEEME. El índice vigente `SHA256SUMS` cubre los archivos de este directorio salvo a sí mismo:

```sh
(cd docs/evidencias/b203 && sha256sum -c SHA256SUMS)
```

Las pruebas de código 0 no certifican todas las señales: con líder ya salido se ensayó expresamente SIGKILL externo, no SIGTERM ni otras señales. Sigue abierta la detección ATA y la variabilidad del arranque `java -version`.
