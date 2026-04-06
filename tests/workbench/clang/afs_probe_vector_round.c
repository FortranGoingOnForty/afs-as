/* Workbench note: this probe exposed the compact SIMD rounding neighborhood
 * Apple clang emits from Neon intrinsics, which led to adding `frintn.4s`,
 * `frintm.4s`, `frintp.4s`, and `frintz.4s`, promoted into
 * `tests/clang_probe/vector_fp_round.c`.
 */
#include <arm_neon.h>

float32x4_t round_nearest(float32x4_t a) { return vrndnq_f32(a); }
float32x4_t round_down(float32x4_t a) { return vrndmq_f32(a); }
float32x4_t round_up(float32x4_t a) { return vrndpq_f32(a); }
float32x4_t round_zero(float32x4_t a) { return vrndq_f32(a); }
