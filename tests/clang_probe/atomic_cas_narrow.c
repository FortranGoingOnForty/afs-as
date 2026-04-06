#include <stdatomic.h>

_Atomic unsigned char g8;
_Atomic unsigned short g16;

unsigned char cas8(unsigned char expected, unsigned char desired) {
    atomic_compare_exchange_strong_explicit(
        &g8,
        &expected,
        desired,
        memory_order_acq_rel,
        memory_order_acquire
    );
    return expected;
}

unsigned short cas16(unsigned short expected, unsigned short desired) {
    atomic_compare_exchange_strong_explicit(
        &g16,
        &expected,
        desired,
        memory_order_acq_rel,
        memory_order_acquire
    );
    return expected;
}
