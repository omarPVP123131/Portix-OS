#include <portix.h>

#define HEAP_START ((void*)0x200000000000ULL)
#define HEAP_SIZE  (1024 * 1024)  /* 1 MB initial heap */
#define HEAP_MAGIC 0xDEADBEEF  /* Magic number to detect corrupted blocks */

typedef struct Block {
    u32 magic;      /* Magic number for corruption detection */
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
    heap_base->magic = HEAP_MAGIC;
    heap_base->size = HEAP_SIZE - sizeof(Block);
    heap_base->free = 1;
    heap_base->next = NULL;
    brk((char*)base + HEAP_SIZE);
}

static int is_valid_block(Block *block) {
    if (!block) return 0;
    if (block->magic != HEAP_MAGIC) return 0;  /* Corrupted block */
    return 1;
}

void *malloc(size_t size) {
    if (size == 0) return NULL;
    if (size > HEAP_SIZE) return NULL;  /* Too large */
    if (!heap_base) heap_init();
    if (!heap_base) return NULL;

    size = (size + 7) & ~7;

    Block *cur = heap_base;
    while (cur) {
        if (!is_valid_block(cur)) {
            /* Heap corruption detected - return NULL */
            return NULL;
        }
        
        if (cur->free && cur->size >= size) {
            if (cur->size >= size + sizeof(Block) + 8) {
                Block *new = (Block*)((char*)(cur + 1) + size);
                
                /* Validate that new block is within bounds */
                if ((char*)new + sizeof(Block) > (char*)heap_base + HEAP_SIZE) {
                    return NULL;  /* Out of bounds */
                }
                
                new->magic = HEAP_MAGIC;
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
    
    /* Validate block before freeing */
    if (!is_valid_block(block)) {
        /* Double-free or heap corruption - ignore */
        return;
    }
    
    if (block->free) {
        /* Already freed - potential double-free */
        return;
    }
    
    block->free = 1;

    Block *cur = heap_base;
    while (cur && cur->next) {
        if (!is_valid_block(cur) || !is_valid_block(cur->next)) {
            /* Heap corruption - stop coalescing */
            return;
        }
        
        if (cur->free && cur->next->free) {
            cur->size += sizeof(Block) + cur->next->size;
            cur->next = cur->next->next;
        } else {
            cur = cur->next;
        }
    }
}

void *calloc(size_t nmemb, size_t size) {
    /* Check for overflow */
    if (size > 0 && nmemb > HEAP_SIZE / size) return NULL;
    
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
    
    /* Validate block */
    if (!is_valid_block(block)) return NULL;
    
    if (block->size >= size) return ptr;

    void *new = malloc(size);
    if (!new) return NULL;
    size_t copy = block->size < size ? block->size : size;
    memcpy(new, ptr, copy);
    free(ptr);
    return new;
}
