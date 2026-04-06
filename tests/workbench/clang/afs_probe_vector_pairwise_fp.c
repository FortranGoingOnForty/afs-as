/* Workbench note: this probe exposed the practical FP vector pairwise min/max
 * family from Apple clang, which led to adding `fmaxp.4s` and `fminp.4s`
 * and promoting the case into `tests/clang_probe/vector_pairwise_fp.c`.
 */
#include <arm_neon.h>

float32x4_t pairwise_max(float32x4_t a, float32x4_t b) { return vpmaxq_f32(a, b); }
float32x4_t pairwise_min(float32x4_t a, float32x4_t b) { return vpminq_f32(a, b); }
