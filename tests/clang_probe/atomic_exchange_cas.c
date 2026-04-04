#include <stdatomic.h>

_Atomic int gi;
_Atomic long gl;

int swap_acqrel(int x) {
    return atomic_exchange_explicit(&gi, x, memory_order_acq_rel);
}

long swap64_acqrel(long x) {
    return atomic_exchange_explicit(&gl, x, memory_order_acq_rel);
}

int cas_acqrel(int expected, int desired) {
    atomic_compare_exchange_strong_explicit(
        &gi,
        &expected,
        desired,
        memory_order_acq_rel,
        memory_order_acquire
    );
    return expected;
}

long cas64_acqrel(long expected, long desired) {
    atomic_compare_exchange_strong_explicit(
        &gl,
        &expected,
        desired,
        memory_order_acq_rel,
        memory_order_acquire
    );
    return expected;
}
