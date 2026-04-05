/* Workbench note: this tail rounding probe exposed the remaining SIMD FP
 * rounding siblings Apple clang emits from Neon intrinsics, which led to
 * adding `frinta.4s` and `frinti.4s`, promoted into
 * `tests/clang_probe/vector_fp_round_tail.c`.
 */
#include <arm_neon.h>

float32x4_t round_away(float32x4_t a) { return vrndaq_f32(a); }
float32x4_t round_current(float32x4_t a) { return vrndiq_f32(a); }
