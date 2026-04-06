#include <arm_neon.h>

float32x4_t swap_halves_tbl(float32x4_t a) {
    static const uint8_t idx_data[16] = {8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7};
    uint8x16_t idx = vld1q_u8(idx_data);
    uint8x16x2_t table = {{vreinterpretq_u8_f32(a), vreinterpretq_u8_f32(a)}};
    return vreinterpretq_f32_u8(vqtbl2q_u8(table, idx));
}

float32x4_t blend_even_tbl(float32x4_t a, float32x4_t b) {
    static const uint8_t idx_data[16] = {0, 1, 2, 3, 20, 21, 22, 23, 8, 9, 10, 11, 28, 29, 30, 31};
    uint8x16_t idx = vld1q_u8(idx_data);
    uint8x16x2_t table = {{vreinterpretq_u8_f32(a), vreinterpretq_u8_f32(b)}};
    return vreinterpretq_f32_u8(vqtbl2q_u8(table, idx));
}
