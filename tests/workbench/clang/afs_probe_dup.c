/* Workbench note: this minimal probe was the first confirmation that Apple
 * clang emits `dup.4s` for lane-splat code, which seeded the later full DUP
 * family slice.
 */
#include <arm_neon.h>
float32x4_t splat_lane(float32x4_t a) {
    return vdupq_laneq_f32(a, 2);
}
