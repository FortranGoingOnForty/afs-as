#include <stdatomic.h>

_Atomic int g;

int add_and_fetch(int x) {
    return atomic_fetch_add_explicit(&g, x, memory_order_acq_rel) + x;
}

int load_then_store(int x) {
    int y = atomic_load_explicit(&g, memory_order_acquire);
    atomic_store_explicit(&g, x + y, memory_order_release);
    return y;
}
