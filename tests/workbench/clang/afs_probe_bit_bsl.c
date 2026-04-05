#include <arm_neon.h>
uint8x16_t sel_mask(uint8x16_t a, uint8x16_t b, uint8x16_t m) {
    return vbslq_u8(m, a, b);
}
uint8x16_t bit_mask(uint8x16_t a, uint8x16_t b, uint8x16_t m) {
    return vbitq_u8(a, b, m);
}
uint8x16_t bif_mask(uint8x16_t a, uint8x16_t b, uint8x16_t m) {
    return vbifq_u8(a, b, m);
}
