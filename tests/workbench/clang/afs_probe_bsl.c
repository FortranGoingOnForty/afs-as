#include <arm_neon.h>
float32x4_t blend_mask(float32x4_t a, float32x4_t b) {
    uint32x4_t mask = {0xffffffffu, 0u, 0xffffffffu, 0u};
    return vbslq_f32(mask, a, b);
}
