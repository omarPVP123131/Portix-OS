#!/usr/bin/env python3
"""Sonda permanente de depuración (ciclo de mantenimiento v6.x).

Verifica en QEMU headless, contra el Portix OS real, la cadena completa del
depurador de Ring 3 `/bin/dbg`:

  1. identidad y capacidades del binario;
  2. símbolos del ELF del objetivo resueltos por `/proc/<pid>/symbols`;
  3. desensamblado x86-64 con simbolización de destinos (leyendo la memoria
     del propio proceso objetivo vía PTRACE_PEEKTEXT);
  4. volcado de memoria del objetivo;
  5. puntos de parada INT3 persistentes por IMAGEN del programa
     (`/tmp/dbg-<imagen>.bps`): SET -> VERIFIED (rearme) -> RESTORED;
  6. vigilancia por hardware sobre DR0-DR3: ARM -> LIST -> CLEAR;
  7. /bin/wp-demo: disparo REAL del #DB (watchpoint), del #BP (INT3) y parcheo
     de código con PEEKTEXT/POKEDATA, con el relato del kernel en la consola;
  8. sesión de depuración interactiva (`dbg --session`): `fork` + `PTRACE_TRACEME`
     + `execve`, con PARADA del objetivo notificada al trazador por `waitpid`
     (WIFSTOPPED), punto de parada simbólico alcanzado sobre el proceso real,
     restauración del byte original, REBOBINADO del RIP y salida limpia;
  9. control de ejecución del objetivo: paso a paso con el Trap Flag real del
     procesador (`step`), paso a NIVEL de llamada con breakpoint temporal en la
     dirección de retorno (`next`), pila de llamadas (`bt`) y lectura de la
     memoria del objetivo (`mem`) desde la consola de la sesión;
 10. WATCHPOINTS CON EXPRESIÓN (`dbg --wcondtest`): la misma condición aplicada a
     los accesos de DATOS vigilados por hardware (DR0-DR3), con la dirección
     derivada del propio código del objetivo; condición falsa (el kernel calla),
     condición encadenada (dos activas a la vez, el trazador despierta sólo por
     la verdadera) y `ignore 1` (el acierto se consume sin notificar);
 11. condiciones CONJUNTAS (dos cláusulas unidas por Y lógico) y disparo por RANGO
     sobre la dirección vigilada (`dbg --rangetest`): el rango `A <= addr < B` se
     expresa como dos cláusulas sobre el pseudo-registro `addr`, de modo que el
     kernel filtra los accesos que caen fuera del tramo vigilado;
 12. breakpoints CONDICIONALES evaluados por el propio KERNEL (`dbg --condtest`)
     sobre puntos de parada de EJECUCIÓN por hardware (DR7 R/W = 00): con la
     condición falsa el kernel reanuda sin despertar al trazador (auto-contínuo
     con re-armado de la ranura tras una instrucción) y con la verdadera detiene
     al objetivo y lo notifica;
 13. lenguaje GENERAL de condiciones (`dbg --ortest`): hasta CUATRO cláusulas
     repartidas en DOS grupos, unidas por Y lógico dentro de cada grupo y por O
     lógico entre grupos. La sonda comprueba la disyunción `x == A || x == B`,
     la conjunción de tres cláusulas, el rango con guarda `A <= addr < B && rsp
     != 0` y la forma mixta `(A <= addr < B) || addr == C`, que despierta al
     trazador por un acceso que el rango puro filtraba en silencio. Todo con los
     contadores del kernel (aciertos/saltos) y el valor real leído en memoria.

El objetivo de las pruebas es `self` (el propio depurador), de modo que la
sonda no depende de qué proceso ocupe cada PID en la sesión.

Uso:  python3 scripts/dbg_probe.py
Salida: 0 si TODAS las comprobaciones pasan, 1 en caso contrario.
"""
import sys, os, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner


def main():
    r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
    r.start_qemu()
    booted, log = r.wait_for_pattern("present done", timeout=90.0)
    if booted:  # [b192 / T-016] init se carga despues de `present done`
        booted, log = r.wait_for_pattern("init process PID=1 registered", timeout=30.0)
    print(f"[DBG-PROBE] arranque del sistema: {booted}")
    if not booted:
        print(log[-3000:])
        r.close()
        return 1
    time.sleep(0.5)
    idx0 = r.get_log_len()
    r.send_command("")
    prompt, _ = r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)
    print(f"[DBG-PROBE] consola interactiva: {prompt}")
    if not prompt:
        r.close()
        return 1

    pruebas = []

    def cmd(orden, patron, etiqueta, t=30.0, volcar=False):
        idx = r.get_log_len()
        r.send_command(orden)
        ok, out = r.wait_for_pattern(patron, start_idx=idx, timeout=t)
        pruebas.append((etiqueta, ok))
        print(f"  [{'PASS' if ok else 'FALLO'}] {etiqueta:<44} -> {patron[:46]}")
        if not ok:
            print("         (salida):", repr(out[-400:]))
        if volcar:
            for l in out.splitlines():
                if l.strip().startswith("0x") or "[DISASM" in l:
                    print("         " + l.strip()[:120])
        return out

    # Carga de trabajo real en Ring 3 + identidad del depurador
    r.send_command("run yes")
    time.sleep(1.2)
    cmd("run dbg --version", "puntos de parada    : INT3 persistente",
        "identidad y capacidades del depurador")

    # Símbolos del ELF del objetivo
    cmd("run dbg self symbols main", "main", "símbolos del ELF vía /proc/<pid>/symbols")

    # Desensamblado simbólico sobre la memoria real del objetivo
    cmd("run dbg self disasm main 10", "DESENSAMBLADO SIMBOLICO",
        "desensamblado simbólico de 10 instrucciones", volcar=True)

    # Volcado de memoria del objetivo
    cmd("run dbg self peek 0x20000000 32", "VOLCADO DE MEMORIA DEL OBJETIVO",
        "lectura de la memoria del proceso objetivo")

    # Puntos de parada persistentes por imagen del programa
    cmd("run dbg --break self main", "[BP-SET-PASS]", "breakpoint INT3 instalado en la imagen")
    cmd("run dbg --bp-list self", "[BP-VERIFIED]", "breakpoint persistente verificado (rearme)")
    cmd("run dbg --bp-clear self", "[BP-RESTORED]", "byte original restaurado y verificado")

    # Vigilancia por hardware (DR0-DR3)
    cmd("run dbg --watch self 0x20003000 8 w", "[WATCH-ARMED]", "watchpoint de hardware armado")
    cmd("run dbg --watch-list self", "[WATCH-VERIFIED]", "watchpoint leído del CPU")
    cmd("run dbg --watch-clear self", "[WATCH-CLEARED]", "watchpoint desarmado")

    # Disparo real de las excepciones de depuración en el silicio (wp-demo)
    cmd("run wp-demo", "[DBGSUITE-PASS]", "excepciones #DB/#BP disparadas en el CPU")
    r.send_command("")
    time.sleep(0.8)
    with r.log_lock:
        for l in r.log.splitlines():
            if any(k in l for k in ("[WATCHPOINT]", "[BREAKPOINT]", "[SINGLE-STEP]", "[DBG] #DB")):
                print("         kernel:", l.strip()[:120])

    # Sesión de depuración interactiva COMPLETA (`dbg --session`): el trazador
    # para al objetivo en `execve`, instala un breakpoint simbólico sobre `<main>`,
    # lo alcanza, restaura el byte original, rebobina el RIP y el objetivo
    # termina por sí mismo con código 0.
    out = cmd("run dbg --session hello", "[SESSION-PASS]",
              "sesión completa: exec-stop, breakpoint, restauración, RIP y salida", t=45.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, clave in (
        ("parada de execve notificada por waitpid (WIFSTOPPED)", "[SESSION-EXEC-STOP]"),
        ("breakpoint simbólico alcanzado sobre <main>", "[SESSION-BP-HIT]"),
        ("byte 0xCC retirado y verificado por PEEKTEXT", "[SESSION-BP-RESTORED]"),
        ("RIP rebobinado a la instrucción original", "[SESSION-RIP-REWIND]"),
        ("objetivo reanudado y finalizado con código 0", "[SESSION-EXIT]"),
        ("sesión de depuración completa verificada", "[SESSION-PASS]"),
    ):
        okk = clave in ses
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {clave}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-", "[TRAP-STOP]", "[BREAKPOINT] PID=")):
            print("         " + l.strip()[:120])

    # Control de ejecución del objetivo desde la consola de la sesión.
    cmd("run dbg --session hello main step,step,step,next,bt,mem 0x20000000 16", "[SESSION-PASS]",
        "control de ejecución: step, next, bt y mem sobre el objetivo vivo", t=60.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    conteo = {
        "pasos": ses.count("[SESSION-STEP]"),
        "resumen": "[SESSION-CMD-RESUMEN] comandos ejecutados=6 de 6" in ses,
        "llamada": "llamada completada: parada en el retorno" in ses,
        "bt": "[SESSION-BT]   #1" in ses,
        "mem": "[SESSION-MEM] 0x0000000020000000" in ses,
        "pass": "[SESSION-PASS]" in ses,
    }
    for etiqueta, okk in (
        ("dos pasos a paso de instrucción ejecutados (TF real)", conteo["pasos"] >= 2),
        ("paso a nivel de llamada detenido en el retorno (next)", conteo["llamada"]),
        ("pila de llamadas con más de un marco (bt)", conteo["bt"]),
        ("memoria del objetivo leída por PEEKTEXT (mem)", conteo["mem"]),
        ("los seis comandos de la sesión se ejecutaron", conteo["resumen"]),
        ("sesión con comandos certificada", conteo["pass"]),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-STEP]", "[SESSION-NEXT] llamada", "[SESSION-BT]   #", "[SESSION-MEM]", "[SESSION-CMD-RESUMEN]")):
            print("         " + l.strip()[:120])

    # Breakpoints condicionales evaluados por el kernel (`dbg --condtest`).
    cmd("run dbg --condtest hello", "[SESSION-COND-PASS]",
        "breakpoints condicionales: condición evaluada por el kernel", t=60.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, okk in (
        ("punto de parada de EJECUCIÓN por hardware disparado",
         "[BREAKPOINT-HW]" in ses),
        ("condición falsa: el kernel reanuda sin notificar (saltos=1)",
         "-> FALSA (saltos=1)" in ses and "[SESSION-COND-SKIP]" in ses),
        ("auto-contínuo con re-armado de la ranura del hardware",
         "re-armada tras el auto-cont" in ses),
        ("condición verdadera: parada notificada (aciertos=1)",
         "-> CIERTA (aciertos=1)" in ses and "[SESSION-COND-HIT]" in ses),
        ("ensayo de condiciones certificado", "[SESSION-COND-PASS]" in ses),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-COND", "[BPCOND]", "[BREAKPOINT-HW]")):
            print("         " + l.strip()[:120])

    # Watchpoints con expresión evaluados por el kernel (`dbg --wcondtest`).
    cmd("run dbg --wcondtest hello", "[SESSION-WCOND-PASS]",
        "watchpoints con expresión: condición evaluada sobre accesos de datos", t=60.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, okk in (
        ("vigilancia de ESCRITURA disparada por el hardware",
         "[SESSION-WCOND-ARMED]" in ses and "[WATCHPOINT] PID=" in ses),
        ("condición falsa sobre datos: el kernel calla (saltos=1)",
         "saltos=1): reanudado por el kernel sin notificar" in ses),
        ("condiciones ENCADENADAS: la de código descartada, la de datos despierta",
         "aciertos=0 saltos=1" in ses and "[SESSION-WCOND-HIT]" in ses),
        ("valor observado en memoria coincide con el PID del objetivo",
         "[SESSION-WCOND-VALUE]" in ses and "(PID del objetivo" in ses),
        ("`ignore 1` consume el acierto sin notificar",
         "[SESSION-WCOND-IGNORE-SKIP]" in ses and "IGNORADA (ignore restante=0)" in ses),
        ("ensayo de watchpoints certificado", "[SESSION-WCOND-PASS]" in ses),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-WCOND", "[BPCOND] PID=")):
            print("         " + l.strip()[:120])

    # Condiciones conjuntas y rango evaluados por el kernel (`dbg --rangetest`).
    cmd("run dbg --rangetest hello", "[SESSION-RANGE-PASS]",
        "condiciones conjuntas y rango: dos cláusulas y filtrado por tramo", t=60.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, okk in (
        ("conjunción verdadera evaluada por el kernel (aciertos=1)",
         "&& rsi==0x0000000000000057" in ses and "[SESSION-RANGE-HIT]" in ses),
        ("conjunción falsa: el kernel calla (2ª cláusula imposible)",
         "rsi==0x0000000000001057" in ses and "[SESSION-RANGE-SKIP]" in ses),
        ("rango: acceso FUERA del tramo filtrado por el kernel (saltos=1)",
         "vigilancia fuera del rango: aciertos=0 saltos=1" in ses),
        ("rango: acceso DENTRO del tramo despierta al trazador (aciertos=1)",
         "vigilancia dentro del rango: aciertos=1 saltos=0" in ses),
        ("valor escrito en memoria coincide con el RSP real del objetivo",
         "[SESSION-RANGE-VALUE]" in ses and "(RSP del objetivo" in ses),
        ("ensayo de conjunción y rango certificado", "[SESSION-RANGE-PASS]" in ses),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-RANGE", "[BPCOND] PID=")):
            print("         " + l.strip()[:120])

    # Lenguaje GENERAL de condiciones: hasta cuatro cláusulas repartidas en dos
    # grupos (Y dentro del grupo, O entre grupos), evaluado por el KERNEL
    # (`dbg --ortest`). Un rango son dos cláusulas del mismo grupo; una
    # disyunción son dos grupos de una; `(A <= x < B) || x == C` combina ambos.
    cmd("run dbg --ortest hello", "[SESSION-OR-PASS]",
        "lenguaje general: disyuncion, triple Y, rango con guarda y rango O exacto", t=90.0)
    r.send_command("")
    time.sleep(4.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, okk in (
        ("disyuncion `x == A || x == B` (2 grupos, contadores del kernel)",
         "[SESSION-OR-DISJUNCTION]" in ses and "[SESSION-OR-STATS] disyunc" in ses
         and "aciertos=1 saltos=0" in ses),
        ("conjuncion de TRES clausulas en un solo grupo",
         "[SESSION-OR-TRIPLE]" in ses and "[SESSION-OR-STATS] tres cl" in ses
         and "aciertos=1 saltos=0" in ses),
        ("rango con guarda `rsp != 0` sobre datos vigilados",
         "[SESSION-OR-GUARDED]" in ses and "[SESSION-OR-STATS] rango con guarda" in ses),
        ("`(min<=addr<max) || addr==marco`: el acceso antes filtrado despierta",
         "[SESSION-OR-RANGE-OR]" in ses and "[SESSION-OR-HIT-EXACT]" in ses),
        ("segunda parada DENTRO del tramo notificada al trazador",
         "[SESSION-OR-HIT-RANGE]" in ses and "notificada=true" in ses),
        ("valor escrito == RSP real del objetivo, aciertos=2 saltos=0",
         "[SESSION-OR-VALUE]" in ses and "rango o exacto: aciertos=2 saltos=0" in ses),
        ("ensayo del lenguaje general certificado", "[SESSION-OR-PASS]" in ses),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if any(k in l for k in ("[SESSION-OR", "[BPCOND] PID=")):
            print("         " + l.strip()[:120])

    # [FRENTE C] Búfer completo repartido entre las CUATRO ranuras DR0-DR3
    # (`dbg --buftest`): cada parada se atribuye con el DR6 real al trozo del
    # búfer tocado; la mitad sin ranura no detiene al objetivo; y las ranuras
    # de un proceso muerto no quedan fantasma en el CPU.
    cmd("run dbg --buftest hello", "[SESSION-BUF-PASS]",
        "bufer de 32 B repartido entre DR0-DR3 con atribucion por DR6", t=90.0)
    r.send_command("")
    time.sleep(3.0)
    with r.log_lock:
        ses = r.log
    for etiqueta, okk in (
        ("cuatro ranuras armadas y confirmadas por el kernel",
         "[SESSION-BUF-ARMED] 4 ranuras armadas sobre el búfer (4 confirmadas" in ses
         or "[SESSION-BUF-ARMED] 4 ranuras armadas sobre el b" in ses),
        ("las cuatro ranuras dispararon (mascara DR=0b1111), todas dentro del bufer",
         "ranuras distintas que dispararon=4 (m" in ses and "todas dentro del b" in ses
         and "=true salida del objetivo=Some(0)" in ses),
        ("el kernel atribuye cada #DB a su ranura (traza con DR6 y direccion vigilada)",
         "ranura=DR0 vigilada=" in ses and "ranura=DR3 vigilada=" in ses),
        ("la mitad del bufer sin ranura no detiene al objetivo",
         "[SESSION-BUF-SILENT]" in ses and "(atribuidas a la mitad baja=0)" in ses),
        ("ensayo del bufer repartido certificado", "[SESSION-BUF-PASS]" in ses),
    ):
        pruebas.append((etiqueta, okk))
        print(f"  [{'PASS' if okk else 'FALLO'}] {etiqueta:<44} -> {'OK' if okk else 'sin evidencia'}")
    for l in ses.splitlines():
        if "[SESSION-BUF" in l:
            print("         " + l.strip()[:120])
    cmd("run dbg --watch self 0x20003000 32 w", "repartido en 4 ranuras (activas en el CPU=4)",
        "--watch de 32 B: cuatro ranuras reales en el CPU (sin ranuras fantasma)")
    cmd("run dbg --watch-clear self", "[WATCH-CLEARED]", "bufer repartido desarmado")
    cmd("run dbg --watch self 0x20003000 64 w", "necesita m", "64 B rechazados: el hardware no da mas de 32 B")

    # [T-045] INVARIANTE DE LA CONSOLA: la serie del invitado debe transportar
    # SÓLO texto. Un fallo intermitente de esta sonda resultó no ser del
    # depurador sino de la consola: en la corrida mala llegaron 21,5 MB con
    # un 88,7 % de bytes NUL y memoria cruda intercalada, y las pruebas que
    # buscan una cadena literal ("búfer de 32 B") fallaron aunque el
    # depurador hubiera hecho su trabajo. Esta comprobación tiene que doler
    # justo cuando la consola se ensucia, no 40 pruebas después.
    with r.log_lock:
        serie_mem = r.log
    bserie = serie_mem.encode("latin1", "replace")
    nulos = bserie.count(0)
    no_impr = sum(1 for c in bserie if c < 32 and c not in (9, 10, 13, 27))
    bultos = serie_mem.count("[SERIE-BULTO]")
    print(f"  [CONSOLA] bytes={len(bserie)} NUL={nulos} no_imprimibles={no_impr} bultos={bultos}")
    pruebas.append(("la consola serie transporta sólo texto (ni NUL ni memoria cruda)",
                    nulos == 0 and no_impr <= 64))
    pruebas.append(("ninguna escritura suelta de >=256 B en la consola", bultos == 0))

    ok = sum(1 for _, k in pruebas if k)
    # [T-044] La serie completa del invitado se guarda SIEMPRE: sin ella un fallo
    # intermitente sólo deja «sin evidencia» y no hay forma de saber qué fase se
    # cortó ni por qué (la primera recertificación de b203 se quedó sin poder
    # diagnosticarlo por esto).
    try:
        import os as _os
        _destino = _os.path.join("build", "dbg_probe_serie.log")
        with r.log_lock:
            _serie = r.log
        with open(_destino, "w", encoding="utf-8", errors="replace") as _fh:
            _fh.write(_serie)
        print(f"  [INFO] serie completa del invitado en {_destino}")
    except Exception as _e:  # noqa: BLE001
        print(f"  [INFO] no se pudo guardar la serie: {_e}")
    print("\n" + "-" * 74)
    print(f"  SONDA DE DEPURACIÓN: {ok}/{len(pruebas)} comprobaciones PASS")
    if ok == len(pruebas):
        print("  [DBG-PROBE-PASS] cadena de depuración de Ring 3 verificada en runtime")
    else:
        print("  [DBG-PROBE-FAIL] alguna comprobación no se cumplió")
    print("-" * 74)
    r.close()
    return 0 if ok == len(pruebas) else 1


if __name__ == "__main__":
    sys.exit(main())
