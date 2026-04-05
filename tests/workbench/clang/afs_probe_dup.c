#include <arm_neon.h>
float32x4_t splat_lane(float32x4_t a) {
    return vdupq_laneq_f32(a, 2);
}
