#include <arm_neon.h>

uint8x16_t pairwise_max_u8(uint8x16_t a, uint8x16_t b) {
    return vpmaxq_u8(a, b);
}

uint8x16_t pairwise_min_u8(uint8x16_t a, uint8x16_t b) {
    return vpminq_u8(a, b);
}

int8x16_t pairwise_max_s8(int8x16_t a, int8x16_t b) {
    return vpmaxq_s8(a, b);
}

int8x16_t pairwise_min_s8(int8x16_t a, int8x16_t b) {
    return vpminq_s8(a, b);
}
