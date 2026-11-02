#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc < 4) {
        printf("Uso: %s <num1> <op> <num2>\n", argv[0] ? argv[0] : "calc_c");
        printf("Operadores soportados: +  -  x  /\n");
        printf("Ejemplo: calc_c 25 + 17\n");
        return 1;
    }

    int a = atoi(argv[1]);
    const char *op = argv[2];
    int b = atoi(argv[3]);
    int res = 0;

    if (strcmp(op, "+") == 0) {
        res = a + b;
    } else if (strcmp(op, "-") == 0) {
        res = a - b;
    } else if (strcmp(op, "x") == 0 || strcmp(op, "*") == 0) {
        res = a * b;
    } else if (strcmp(op, "/") == 0) {
        if (b == 0) {
            printf("[ERROR] Division por cero rechazada.\n");
            return 2;
        }
        res = a / b;
    } else {
        printf("[ERROR] Operador no valido: %s\n", op);
        return 3;
    }

    printf("[CALC-C] %d %s %d = %d\n", a, op, b, res);
    return 0;
}
