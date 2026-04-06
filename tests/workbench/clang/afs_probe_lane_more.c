/* Workbench note: this lane-transfer exploration showed the concrete Apple
 * clang output for half extraction and lane combine/set operations, confirming
 * that our existing `ext.16b`, `mov.d`, and `mov.s` coverage matched the real
 * codegen shapes.
 */
#include <arm_neon.h>
float32x2_t low_half(float32x4_t a) { return vget_low_f32(a); }
float32x2_t high_half(float32x4_t a) { return vget_high_f32(a); }
float32x4_t combine_halves(float32x2_t a, float32x2_t b) { return vcombine_f32(a, b); }
float32x4_t set_lane(float32x4_t a, float x) { return vsetq_lane_f32(x, a, 2); }
