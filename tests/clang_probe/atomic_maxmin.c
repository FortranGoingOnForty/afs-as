#include <stdatomic.h>

_Atomic int g;

int fetch_max_builtin(int x) {
    return __c11_atomic_fetch_max(&g, x, memory_order_acq_rel);
}

int fetch_min_builtin(int x) {
    return __c11_atomic_fetch_min(&g, x, memory_order_acq_rel);
}
