#include <arm_neon.h>

float32x4_t zip_lo(float32x4_t a, float32x4_t b) {
    return vzip1q_f32(a, b);
}

float32x4_t zip_hi(float32x4_t a, float32x4_t b) {
    return vzip2q_f32(a, b);
}

float32x4_t uzp_lo(float32x4_t a, float32x4_t b) {
    return vuzp1q_f32(a, b);
}

float32x4_t uzp_hi(float32x4_t a, float32x4_t b) {
    return vuzp2q_f32(a, b);
}

float32x4_t trn_lo(float32x4_t a, float32x4_t b) {
    return vtrn1q_f32(a, b);
}

float32x4_t trn_hi(float32x4_t a, float32x4_t b) {
    return vtrn2q_f32(a, b);
}
