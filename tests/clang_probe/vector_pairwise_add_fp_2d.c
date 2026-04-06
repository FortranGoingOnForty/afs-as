#include <arm_neon.h>

float64x2_t pairwise_add_f64(float64x2_t a, float64x2_t b) {
    return vpaddq_f64(a, b);
}
