/* Prueba real de PCC v1.2: union, static local y asignacion de struct por valor.
 * Compilada y ejecutada DENTRO de Portix por /bin/portix-cc. Cada linea impresa
 * la comprueba la suite headless; si el compilador falla, la suite falla. */
struct Vec3 { int x; int y; int z; };

union Celda {
    int entero;
    char letras[8];
    struct Vec3 *ptr;
};

int contador(void) {
    static int veces = 100;   /* se inicializa una sola vez */
    veces += 1;
    return veces;
}

int siguiente_id(void) {
    static int id;            /* sin inicializador: cero */
    return ++id;
}

struct Vec3 origen = { 1, 2, 3 };

int main(int argc, char **argv) {
    union Celda c;
    c.entero = 0x41424344;    /* "DCBA" en little endian */
    printf("union: sizeof=%d entero=%x letras[0]='%c' letras[3]='%c'\n", sizeof(union Celda), c.entero, c.letras[0], c.letras[3]);
    c.letras[0] = 'Z';
    printf("union solapada: entero=%x\n", c.entero);

    int a = contador(); int b = contador(); int d = contador();
    int i1 = siguiente_id(); int i2 = siguiente_id(); int i3 = siguiente_id();
    printf("static local: %d %d %d ids=%d,%d,%d\n", a, b, d, i1, i2, i3);

    struct Vec3 v;
    v = origen;               /* copia por valor (rep movsb) */
    v.x = 10;
    struct Vec3 w = v;        /* inicializacion por copia */
    w.z = 30;
    printf("struct por valor: origen=(%d,%d,%d) v=(%d,%d,%d) w=(%d,%d,%d)\n", origen.x, origen.y, origen.z, v.x, v.y, v.z, w.x, w.y, w.z);

    struct Vec3 arr[3];
    arr[0] = w; arr[1] = v; arr[2] = origen;
    int suma = 0;
    for (int i = 0; i < 3; i++) suma += arr[i].x + arr[i].y + arr[i].z;
    printf("arreglo de structs copiados: suma=%d\n", suma);
    puts("[PCC12-PASS] union/static/struct-por-valor verificados");
    return 0;
}
