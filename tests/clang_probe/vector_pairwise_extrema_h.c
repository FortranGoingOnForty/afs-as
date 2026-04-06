#include <arm_neon.h>

uint16x8_t pairwise_max_u16(uint16x8_t a, uint16x8_t b) {
    return vpmaxq_u16(a, b);
}

uint16x8_t pairwise_min_u16(uint16x8_t a, uint16x8_t b) {
    return vpminq_u16(a, b);
}

int16x8_t pairwise_max_s16(int16x8_t a, int16x8_t b) {
    return vpmaxq_s16(a, b);
}

int16x8_t pairwise_min_s16(int16x8_t a, int16x8_t b) {
    return vpminq_s16(a, b);
}
