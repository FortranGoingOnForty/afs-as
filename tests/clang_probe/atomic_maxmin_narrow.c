#include <stdatomic.h>

_Atomic unsigned char g8;
_Atomic unsigned short g16;

unsigned char max8(unsigned char x) {
    return __c11_atomic_fetch_max(&g8, x, __ATOMIC_ACQ_REL);
}

unsigned char min8(unsigned char x) {
    return __c11_atomic_fetch_min(&g8, x, __ATOMIC_ACQ_REL);
}

unsigned short max16(unsigned short x) {
    return __c11_atomic_fetch_max(&g16, x, __ATOMIC_ACQ_REL);
}

unsigned short min16(unsigned short x) {
    return __c11_atomic_fetch_min(&g16, x, __ATOMIC_ACQ_REL);
}
