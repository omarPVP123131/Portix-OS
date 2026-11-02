/* PCC v1.1: struct, switch, #define, enum, inicializadores */
#include <stdio.h>
#define MAX_PUNTOS 4
#define SALUDO "estructuras"
enum Color { ROJO, VERDE = 5, AZUL };

struct Punto { int x; int y; char etiqueta; };
typedef struct Nodo { int valor; struct Nodo *sig; } Nodo;

struct Punto puntos[MAX_PUNTOS];
int tabla[] = {10, 20, 30, 40, 50};
char nombre[] = "Portix";

int dist2(struct Punto *a, struct Punto *b) {
    int dx = a->x - b->x, dy = a->y - b->y;
    return dx * dx + dy * dy;
}

char *color_a_texto(int c) {
    switch (c) {
        case ROJO: return "rojo";
        case VERDE: return "verde";
        case AZUL: return "azul";
        default: return "desconocido";
    }
}

int main() {
    printf("== PCC v1.1: %s ==\n", SALUDO);
    for (int i = 0; i < MAX_PUNTOS; i++) { puntos[i].x = i * 3; puntos[i].y = i * 4; puntos[i].etiqueta = 'A' + i; }
    struct Punto *p = &puntos[3];
    printf("p=(%d,%d,'%c') dist2(p0,p3)=%d sizeof(struct Punto)=%d\n", p->x, p->y, p->etiqueta, dist2(&puntos[0], p), sizeof(struct Punto));
    Nodo n3; n3.valor = 3; n3.sig = 0;
    Nodo n2; n2.valor = 2; n2.sig = &n3;
    Nodo n1; n1.valor = 1; n1.sig = &n2;
    int suma = 0;
    for (Nodo *it = &n1; it; it = it->sig) suma += it->valor;
    printf("lista enlazada: suma=%d\n", suma);
    int total = 0;
    for (int i = 0; i < 5; i++) total += tabla[i];
    printf("tabla[]: total=%d sizeof=%d nombre=\"%s\" (%d bytes)\n", total, sizeof(tabla), nombre, sizeof(nombre));
    printf("colores: %s %s %s %s (VERDE=%d)\n", color_a_texto(ROJO), color_a_texto(VERDE), color_a_texto(AZUL), color_a_texto(9), VERDE);
    int caidas = 0;
    for (int k = 0; k < 4; k++) {
        switch (k) {
            case 0: caidas += 1;
            case 1: caidas += 10; break;
            case 2: caidas += 100; break;
        }
    }
    printf("switch con caida: %d\n", caidas);
    puts("[PCC11-PASS] struct/switch/define/enum verificados");
    return 0;
}
