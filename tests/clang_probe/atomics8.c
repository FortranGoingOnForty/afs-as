#include <stdatomic.h>

_Atomic unsigned char g8;

unsigned char load_then_store8(unsigned char x) {
    unsigned char y = atomic_load_explicit(&g8, memory_order_acquire);
    atomic_store_explicit(&g8, (unsigned char)(x + y), memory_order_release);
    return y;
}

unsigned char swap8(unsigned char x) {
    return atomic_exchange_explicit(&g8, x, memory_order_acq_rel);
}
