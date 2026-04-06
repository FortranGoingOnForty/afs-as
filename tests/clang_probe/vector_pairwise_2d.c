#include <arm_neon.h>

uint64x2_t pairwise_add_u64(uint64x2_t a, uint64x2_t b) {
    return vpaddq_u64(a, b);
}

int64x2_t pairwise_add_s64(int64x2_t a, int64x2_t b) {
    return vpaddq_s64(a, b);
}

float64x2_t pairwise_max_f64(float64x2_t a, float64x2_t b) {
    return vpmaxq_f64(a, b);
}

float64x2_t pairwise_min_f64(float64x2_t a, float64x2_t b) {
    return vpminq_f64(a, b);
}
