#include <stdatomic.h>

int fence_acquire(int *p) {
    atomic_thread_fence(memory_order_acquire);
    return *p;
}

void fence_release(int *p, int v) {
    *p = v;
    atomic_thread_fence(memory_order_release);
}

int fence_acq_rel(int *p) {
    atomic_thread_fence(memory_order_acq_rel);
    return *p;
}

int fence_seq_cst(int *p) {
    atomic_thread_fence(memory_order_seq_cst);
    return *p;
}
