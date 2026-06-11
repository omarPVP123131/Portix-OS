#include <portix.h>

#define HEAP_START ((void*)0x200000000000ULL)
#define HEAP_SIZE  (1024 * 1024)  /* 1 MB initial heap */

typedef struct Block {
    size_t size;
    int free;
    struct Block *next;
} Block;

static Block *heap_base = NULL;

static void heap_init(void) {
    if (heap_base) return;
    void *base = brk(NULL);
    if ((u64)base < (u64)HEAP_START) {
        base = brk(HEAP_START);
    }
    if (base == (void*)-1) return;
    heap_base = (Block*)base;
    heap_base->size = HEAP_SIZE - sizeof(Block);
    heap_base->free = 1;
    heap_base->next = NULL;
    brk((char*)base + HEAP_SIZE);
}

void *malloc(size_t size) {
    if (size == 0) return NULL;
    if (!heap_base) heap_init();
    if (!heap_base) return NULL;

    size = (size + 7) & ~7;

    Block *cur = heap_base;
    while (cur) {
        if (cur->free && cur->size >= size) {
            if (cur->size >= size + sizeof(Block) + 8) {
                Block *new = (Block*)((char*)(cur + 1) + size);
                new->size = cur->size - size - sizeof(Block);
                new->free = 1;
                new->next = cur->next;
                cur->size = size;
                cur->next = new;
            }
            cur->free = 0;
            return (void*)(cur + 1);
        }
        cur = cur->next;
    }
    return NULL;
}

void free(void *ptr) {
    if (!ptr) return;
    Block *block = (Block*)ptr - 1;
    block->free = 1;

    Block *cur = heap_base;
    while (cur && cur->next) {
        if (cur->free && cur->next->free) {
            cur->size += sizeof(Block) + cur->next->size;
            cur->next = cur->next->next;
        } else {
            cur = cur->next;
        }
    }
}

void *calloc(size_t nmemb, size_t size) {
    size_t total = nmemb * size;
    void *ptr = malloc(total);
    if (ptr) memset(ptr, 0, total);
    return ptr;
}

void _exit(int code) {
    syscall(SYS_EXIT, code, 0, 0, 0, 0);
    __builtin_unreachable();
}

void *realloc(void *ptr, size_t size) {
    if (!ptr) return malloc(size);
    if (size == 0) { free(ptr); return NULL; }

    Block *block = (Block*)ptr - 1;
    if (block->size >= size) return ptr;

    void *new = malloc(size);
    if (!new) return NULL;
    size_t copy = block->size < size ? block->size : size;
    memcpy(new, ptr, copy);
    free(ptr);
    return new;
}
