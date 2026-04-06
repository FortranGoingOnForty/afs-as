#include <stdatomic.h>

_Atomic unsigned char g8;
_Atomic unsigned short g16;

unsigned char add8(unsigned char x) {
    return atomic_fetch_add_explicit(&g8, x, memory_order_acq_rel);
}

unsigned short add16(unsigned short x) {
    return atomic_fetch_add_explicit(&g16, x, memory_order_acq_rel);
}
