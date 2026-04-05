/* Workbench note: this probe exposed the NaN-aware FP vector pairwise family
 * from Apple clang, which led to adding `fmaxnmp.4s` and `fminnmp.4s`
 * and promoting the case into `tests/clang_probe/vector_pairwise_nm.c`.
 */
#include <arm_neon.h>

float32x4_t pairwise_maxnm(float32x4_t a, float32x4_t b) { return vpmaxnmq_f32(a, b); }
float32x4_t pairwise_minnm(float32x4_t a, float32x4_t b) { return vpminnmq_f32(a, b); }
