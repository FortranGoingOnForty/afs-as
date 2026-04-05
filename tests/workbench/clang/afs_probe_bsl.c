/* Workbench note: this initial blend probe showed that a simple `vbslq_f32`
 * shape optimized into existing `rev64.4s` + `trn2.4s` forms, so it was not
 * enough by itself to justify a dedicated vector blend family.
 */
#include <arm_neon.h>
float32x4_t blend_mask(float32x4_t a, float32x4_t b) {
    uint32x4_t mask = {0xffffffffu, 0u, 0xffffffffu, 0u};
    return vbslq_f32(mask, a, b);
}
