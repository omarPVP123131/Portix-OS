#!/usr/bin/env python3
"""
[CICLO 50] Auditoría estricta de las pruebas de /bin/test_posix.

Una prueba que no puede fallar no es una prueba. Este guion recorre cada bloque
`[TEST n]` de userland/bin/test_posix/src/main.rs y lo clasifica:

  TAUTOLOGIA   : imprime "verified" sin ninguna condición (siempre pasa).
  ERROR-GENERICO: el camino de éxito acepta un error sin discriminar
                  (`== -1`, `< 0`, `!= 0`, `is_err()` sobre el resultado del syscall
                  en la rama "verified"). Un -1 genérico no demuestra ENOENT ni
                  EBADF: demuestra "algo falló".
  SIN-RAMA-FALLO: hay condición pero no existe rama que imprima un fallo.
  OK           : condición discriminante con rama de fallo.

Uso:
  python3 scripts/auditoria_pruebas.py            # resumen + listado
  python3 scripts/auditoria_pruebas.py --estricto # código de salida 1 si hay hallazgos
  python3 scripts/auditoria_pruebas.py --json out.json
"""
import re, sys, json, os

RUTA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "userland", "bin", "test_posix", "src", "main.rs")

def cargar():
    with open(RUTA, encoding="utf-8", errors="replace") as f:
        return f.read().split("\n")

# Patrones de "éxito" que en realidad aceptan un error genérico.
GENERICOS = [
    re.compile(r"if\s+[A-Za-z_][A-Za-z0-9_\.]*\s*==\s*-1\s*(\{|&&|\|\|)"),
    re.compile(r"if\s+[A-Za-z_][A-Za-z0-9_\.]*\s*<\s*0\s*(\{|&&|\|\|)"),
    re.compile(r"if\s+[A-Za-z_][A-Za-z0-9_\.]*\s*!=\s*0\s*\{"),
    re.compile(r"if\s+[A-Za-z_][A-Za-z0-9_\.]*\s*<=\s*0\s*\{"),
    re.compile(r"&&\s*[A-Za-z_][A-Za-z0-9_\.]*\s*==\s*-1\b"),
    re.compile(r"&&\s*[A-Za-z_][A-Za-z0-9_\.]*\s*<\s*0\b"),
    re.compile(r"\|\|\s*[A-Za-z_][A-Za-z0-9_\.]*\s*==\s*-1\b"),
]
# Excepciones legítimas: comparaciones con errno concreto (-2, -9, -22, ...) son discriminantes.

def bloques(lineas):
    """Agrupa líneas por número de TEST usando el println!("[TEST n] ...") como ancla.
    El bloque de un test va desde la línea posterior al bloque anterior hasta su último println."""
    marcas = []
    for i, l in enumerate(lineas):
        m = re.search(r'\[TEST (\d+)\]', l)
        if m and "println!" in l:
            marcas.append((i, int(m.group(1))))
    # agrupar índices consecutivos por número
    grupos = {}
    for i, n in marcas:
        grupos.setdefault(n, []).append(i)
    res = []
    nums = sorted(grupos, key=lambda n: grupos[n][0])
    prev_end = 0
    for n in nums:
        idxs = grupos[n]
        ini, fin = idxs[0], idxs[-1]
        # retroceder hasta la apertura de bloque '{' más cercana tras el test anterior
        j = ini
        while j > prev_end and not re.match(r"\s*\{\s*$", lineas[j]) and not re.match(r"\s*//\s*\d+\.", lineas[j]):
            j -= 1
        res.append((n, j, fin, lineas[j:fin + 1]))
        prev_end = fin
    return res

# Excepciones revisadas a mano (ciclo 50). Cada entrada lleva su justificacion.
EXCEPCIONES = {
    1241: "`b != 0` recorre bytes de entropia, no es el resultado de un syscall",
    1296: "`cl != 0` marca fallo (all_ok=false), no exito",
    2296: "poll_key devuelve -1 como centinela documentado de 'sin tecla'; la prueba lo exige exacto",
    2740: "`mremap_moved != 0` elige la direccion vigente, no evalua un error",
    2759: "`code1 == -1` compara si_code con SI_QUEUE (-1), no un errno",
}

def clasificar(n, cuerpo):
    if n in EXCEPCIONES:
        return "OK", "excepcion revisada: " + EXCEPCIONES[n]
    txt = "\n".join(cuerpo)
    # Las retiradas en el ciclo 46 imprimen "PENDIENTE" y la suite no las cuenta como PASS.
    def es_fallo(l):
        ll = l.lower()
        return any(k in ll for k in ("failed", "fallo", "fall\u00f3", "unexpected", "mismatch"))
    exitos = [l for l in cuerpo if "println!" in l and f"[TEST {n}]" in l and "PENDIENTE" not in l and "RESUMEN-DE-EPOCA" not in l and "SIN-SUSTRATO" not in l and "verified" in l.lower() and not es_fallo(l)]
    if not exitos and any("PENDIENTE" in l or "RESUMEN-DE-EPOCA" in l or "SIN-SUSTRATO" in l for l in cuerpo):
        return "PENDIENTE", "retirada en el ciclo 46"
    fallos = [l for l in cuerpo if "println!" in l and f"[TEST {n}]" in l and l not in exitos]
    tiene_if = any(re.search(r"\bif\b|\bmatch\b|\bassert", l) for l in cuerpo)
    if exitos and not tiene_if:
        return "TAUTOLOGIA", "println de éxito sin condición"
    if exitos and tiene_if and not fallos:
        if len(exitos) >= 2:
            return "TAUTOLOGIA", "el veredicto 'verified' se imprime en ambas ramas"
        return "SIN-RAMA-FALLO", "no hay println de fallo"
    # Localizar la condición inmediatamente anterior al println de éxito
    for l in exitos:
        k = cuerpo.index(l)
        ventana = "\n".join(cuerpo[max(0, k - 4):k + 1])
        for pat in GENERICOS:
            m = pat.search(ventana)
            if m:
                # ignorar si la misma condición también exige un errno concreto (p.ej. `== -2 ||`)
                if re.search(r"==\s*-(2|9|13|17|22|38|11|32|4|14|3|7|10|16|18|20|21|24|25|28|29|31|33|34|39|40|61|75|84|88|90|95|98|99|107|111|115)\b", ventana) and "== -1" not in m.group(0):
                    continue
                # -EPERM es literalmente -1: se acepta `== -1` solo si la prueba
                # declara EPERM y el nucleo ya no devuelve -1 generico (ciclo 50).
                if "== -1" in m.group(0) and "EPERM" in l:
                    continue
                return "ERROR-GENERICO", m.group(0).strip()
    return "OK", ""

def main():
    estricto = "--estricto" in sys.argv
    salida_json = None
    if "--json" in sys.argv:
        salida_json = sys.argv[sys.argv.index("--json") + 1]
    lineas = cargar()
    hallazgos = {"TAUTOLOGIA": [], "SIN-RAMA-FALLO": [], "ERROR-GENERICO": [], "OK": [], "PENDIENTE": []}
    for n, ini, fin, cuerpo in bloques(lineas):
        cat, det = clasificar(n, cuerpo)
        hallazgos[cat].append({"test": n, "linea": ini + 1, "detalle": det})
    total = sum(len(v) for v in hallazgos.values())
    print(f"Pruebas analizadas: {total}")
    for cat in ("TAUTOLOGIA", "ERROR-GENERICO", "SIN-RAMA-FALLO", "PENDIENTE", "OK"):
        print(f"  {cat:15s}: {len(hallazgos[cat])}")
    for cat in ("TAUTOLOGIA", "ERROR-GENERICO", "SIN-RAMA-FALLO"):
        if hallazgos[cat]:
            print(f"\n== {cat} ==")
            for h in hallazgos[cat]:
                print(f"  TEST {h['test']:5d}  L{h['linea']:6d}  {h['detalle']}")
    if salida_json:
        with open(salida_json, "w") as f:
            json.dump(hallazgos, f, indent=1)
    malos = len(hallazgos["TAUTOLOGIA"]) + len(hallazgos["ERROR-GENERICO"]) + len(hallazgos["SIN-RAMA-FALLO"])
    print(f"\nAUDITORIA TEST_POSIX: hallazgos={malos}")
    if estricto and malos:
        sys.exit(1)

if __name__ == "__main__":
    main()
