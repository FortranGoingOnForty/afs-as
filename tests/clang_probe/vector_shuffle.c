#include <arm_neon.h>

float32x4_t swap_halves(float32x4_t a) {
    return vreinterpretq_f32_u8(vextq_u8(vreinterpretq_u8_f32(a), vreinterpretq_u8_f32(a), 8));
}

float32x4_t blend_even(float32x4_t a, float32x4_t b) {
    return vtrn2q_f32(vrev64q_f32(a), b);
}
