# T-041 · Paridad BIOS/UEFI restablecida — resumen verificable

**Fecha:** 31 de octubre de 2026 · **Lote:** b203 (en curso) · **Entorno:** QEMU 10.0.13, TCG (sin KVM),
2 vCPU, 512 MiB, OVMF para UEFI y BIOS legacy para el otro arranque.

## 1. El defecto

Bajo UEFI, todo binario propio enlazado en `0x20000000` (`curl`, `init`) moría en su **primera
instrucción**:

```
[EXCEPTION] #PF CR2=0x0 EC=0x5 RIP=0x20005968 PID=2 'curl'
```

Bajo BIOS el mismo `/bin/curl` de la misma imagen arrancaba y sólo fallaba `connect()`.
Era determinista en ambos firmware.

## 2. Cómo se localizó

| Dato | Valor observado | Qué descartó |
|---|---|---|
| `EC=0x5` (bit P=1) | violación de protección, no página ausente | un RIP sin mapear |
| `pte_rip=0x20005027` | `P|RW|U|A` — la escribe `map_page` | un fallo del cargador al crear la PTE |
| bytes reales en `RIP` | `00 00 00 00 00 00 00 00` | que se ejecutara el código del fichero |
| bytes del fichero en ese offset | `E0 04 00 00 BF 89 E2 02` | que el fichero estuviera mal leído |
| físico de la PTE | `0x20005000` = 512 MiB + 20 KiB | RAM: es el **hueco PCI/MMIO** |
| `cero_flags=0xE3` | `P|RW|PS|A|D` en la dirección 0 | página gigante heredada de OVMF (dato secundario) |

`-m 512M` ⇒ la RAM acaba en `0x20000000`. El marco entregado al proceso estaba **fuera de RAM**:
`limpiar()` y las copias de los segmentos se perdían y leer devolvía ceros ⇒ el proceso ejecutaba ceros.

## 3. Causa raíz

`marcos::liberar()` sólo comprobaba `MARCOS_BASE ≤ p < MARCOS_LIMITE`, sin comprobar que el firmware
hubiera declarado ese físico como RAM. Bajo UEFI el mapa es fragmentado (la última región usable acaba
exactamente en `0x20000000`) y alguna ruta liberaba un marco del hueco; `asignar()` lo entregaba luego
como marco de usuario. Bajo BIOS el mismo error no llegaba a materializarse.

## 4. Arreglo

1. **`kernel/src/mem/marcos.rs`** — segundo mapa de bits `existe` con la RAM declarada por el firmware.
   `liberar()` ignora lo que no esté ahí y lo cuenta (`LIBERADOS_IGNORADOS`, publicado en `PROC: exit`
   como `liberados_ignorados=`); `es_marco()` también lo exige.
   *Invariante nueva: el asignador nunca entrega un físico que el firmware no declaró RAM.*
2. **`kernel/src/elf.rs`** — sólo se reutiliza una traducción que ya es de usuario (PTE con `USER` y
   físico dinámico). Antes bastaba con que no fuese gigante y una página de identidad de 4 KiB pasaba
   el filtro.
3. Sin crates nuevas, sin `unwrap` nuevos y sin tocar las zonas protegidas (`boot/`, `linker.ld`,
   `idt.rs`, `isr.asm`, `allocator.rs`).

## 5. Resultado: misma batería en los dos firmware

Imágenes: `portix.img` `fc20a5aa2630e6b1cea9db4a81c0f87b50ffc3233ccde0019a127d57dfd47f6c`,
`portix-uefi.img` `c4ede9b8d69d9f7aeb05010af0d8a10445da6c295c61dc35b8db78af5e29e701`,
fixture `525a3493…073a`.

`scripts/sonda_t035_uefi.py --firmware {uefi,bios} --transporte red`:

| | BIOS legacy | UEFI (OVMF) |
|---|---|---|
| Arranque (mitad superior, escritorio, consola) | PASS | PASS |
| Descarga real por red con `curl` | 1 211 072 B | 1 211 072 B |
| 6 modos de memoria | **6/6** | **6/6** |
| ELF del ext2 idéntico byte a byte | PASS | PASS |
| Traza de `curl` | `[CURL]`×3 · `[CURL-FAIL] err=-111` · `code=1` | idéntica |

## 6. Suites completas y recertificación (misma imagen, sin reconstruir entre pasadas)

- Suite 1: **3814 / 3814**, 0 fallos.
- Suite 2: **3814 / 3814**, 0 fallos.
- Recertificación `--no-build`: **MASTER-PASS 7/7** (auditoría estricta 0 tautologías / 0 errno
  genérico; interactiva `128/128`; Épicas 58/59/60; depurador `62/62`; UEFI `4/4`).
- Controles finales: `unittest` **266/266**, changelog PASS, `sha256sum -c` sin discrepancias.

## 7. Lo que sigue abierto (no se maquilla)

- **T-026**: el contador acumulado dio 675 (fallo, imagen anterior) y 413 / 370 (pasan) en este ciclo,
  con `peor_hueco_us` de 236 948 y 49 744 en dos ejecuciones del mismo binario. Queda **atribuido a la
  variabilidad de la medida bajo TCG**, no cerrado, y el umbral no se relaja.
- **T-035, T-036, T-037, T-038**: siguen abiertos como tickets propios.
- Todo es QEMU: **no hay validación en hardware físico**, y las pruebas UEFI son arranque + batería,
  no una campaña de firmware.
