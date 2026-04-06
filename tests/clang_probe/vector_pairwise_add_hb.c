#include <arm_neon.h>

uint16x8_t pairwise_add_u16(uint16x8_t a, uint16x8_t b) {
    return vpaddq_u16(a, b);
}

int16x8_t pairwise_add_s16(int16x8_t a, int16x8_t b) {
    return vpaddq_s16(a, b);
}

uint8x16_t pairwise_add_u8(uint8x16_t a, uint8x16_t b) {
    return vpaddq_u8(a, b);
}

int8x16_t pairwise_add_s8(int8x16_t a, int8x16_t b) {
    return vpaddq_s8(a, b);
}
