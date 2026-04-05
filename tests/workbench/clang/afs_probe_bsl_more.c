#include <arm_neon.h>
uint8x16_t blend_bytes(uint8x16_t a, uint8x16_t b, uint8x16_t m) {
    return vbslq_u8(m, a, b);
}
uint32x4_t blend_u32(uint32x4_t a, uint32x4_t b, uint32x4_t m) {
    return vbslq_u32(m, a, b);
}
