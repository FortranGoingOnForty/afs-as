#include <stdatomic.h>

_Atomic unsigned char g8;
_Atomic unsigned short g16;

unsigned char or8(unsigned char x) {
    return atomic_fetch_or_explicit(&g8, x, memory_order_acq_rel);
}

unsigned char xor8(unsigned char x) {
    return atomic_fetch_xor_explicit(&g8, x, memory_order_acq_rel);
}

unsigned char and8(unsigned char x) {
    return atomic_fetch_and_explicit(&g8, x, memory_order_acq_rel);
}

unsigned short or16(unsigned short x) {
    return atomic_fetch_or_explicit(&g16, x, memory_order_acq_rel);
}

unsigned short xor16(unsigned short x) {
    return atomic_fetch_xor_explicit(&g16, x, memory_order_acq_rel);
}

unsigned short and16(unsigned short x) {
    return atomic_fetch_and_explicit(&g16, x, memory_order_acq_rel);
}
