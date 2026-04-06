#include <stdatomic.h>

_Atomic unsigned g;

unsigned fetch_or_mask(unsigned x) {
    return atomic_fetch_or_explicit(&g, x, memory_order_acq_rel);
}

unsigned fetch_xor_mask(unsigned x) {
    return atomic_fetch_xor_explicit(&g, x, memory_order_acq_rel);
}

unsigned fetch_and_mask(unsigned x) {
    return atomic_fetch_and_explicit(&g, x, memory_order_acq_rel);
}
