/* Programa de prueba de PCC: criba, ordenacion, cadenas, recursion, argv */
#include <stdio.h>
#include <stdlib.h>

#define N 50
int criba[100];
int movimientos;

void hanoi(int n, int de, int a, int via) {
    if (n == 0) return;
    hanoi(n - 1, de, via, a);
    movimientos++;
    hanoi(n - 1, via, a, de);
}

int suma6(int a, int b, int c, int d, int e, int f) { return a + b + c + d + e + f; }

void invertir(char *s) {
    int i = 0, j = strlen(s) - 1;
    while (i < j) { char t = s[i]; s[i] = s[j]; s[j] = t; i++; j--; }
}

void burbuja(int *v, int n) {
    for (int i = 0; i < n - 1; i++)
        for (int j = 0; j < n - 1 - i; j++)
            if (v[j] > v[j + 1]) { int t = v[j]; v[j] = v[j + 1]; v[j + 1] = t; }
}

int main(int argc, char **argv) {
    printf("== PCC en Portix: programa de prueba ==\n");
    for (int i = 0; i < argc; i++) printf("argv[%d]=%s\n", i, argv[i]);
    int cuenta = 0;
    for (int i = 2; i < 100; i++) {
        if (criba[i]) continue;
        cuenta++;
        if (cuenta <= 10) printf("%d ", i);
        for (int k = i * 2; k < 100; k += i) criba[k] = 1;
    }
    printf("... primos<100: %d\n", cuenta);
    hanoi(10, 1, 3, 2);
    printf("hanoi(10): %d movimientos, suma6=%d\n", movimientos, suma6(1, 2, 3, 4, 5, 6));
    char *txt = malloc(64);
    strcpy(txt, "Portix OS");
    invertir(txt);
    printf("invertida: '%s' cmp=%d\n", txt, strcmp(txt, "SO xitroP"));
    int v[8] = {42, 7, 19, 3, 88, 1, 56, 23};
    burbuja(v, 8);
    for (int i = 0; i < 8; i++) printf("%d%c", v[i], i == 7 ? '\n' : ',');
    int fact = 1; int n = 20;
    while (n > 1) fact *= n--;
    printf("20! = %d (64 bits) atoi(\"-1234\")=%d\n", fact, atoi("-1234"));
    puts("[PCC-PASS] programa C compilado y ejecutado en Portix");
    return 0;
}
