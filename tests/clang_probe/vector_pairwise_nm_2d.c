#include <arm_neon.h>

float64x2_t pairwise_maxnm_f64(float64x2_t a, float64x2_t b) {
    return vpmaxnmq_f64(a, b);
}

float64x2_t pairwise_minnm_f64(float64x2_t a, float64x2_t b) {
    return vpminnmq_f64(a, b);
}
