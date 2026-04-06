#include <stdatomic.h>

_Atomic unsigned short g16;

unsigned short load_then_store16(unsigned short x) {
    unsigned short y = atomic_load_explicit(&g16, memory_order_acquire);
    atomic_store_explicit(&g16, (unsigned short)(x + y), memory_order_release);
    return y;
}

unsigned short swap16(unsigned short x) {
    return atomic_exchange_explicit(&g16, x, memory_order_acq_rel);
}
