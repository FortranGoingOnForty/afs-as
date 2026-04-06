/* Workbench note: this probe exposed the practical integer vector pairwise
 * extrema family from Apple clang, which led to adding `umaxp.4s`,
 * `uminp.4s`, `smaxp.4s`, and `sminp.4s` and promoting the case into
 * `tests/clang_probe/vector_pairwise_extrema.c`.
 */
#include <arm_neon.h>

uint32x4_t pairwise_max_u32(uint32x4_t a, uint32x4_t b) { return vpmaxq_u32(a, b); }
uint32x4_t pairwise_min_u32(uint32x4_t a, uint32x4_t b) { return vpminq_u32(a, b); }
int32x4_t pairwise_max_s32(int32x4_t a, int32x4_t b) { return vpmaxq_s32(a, b); }
int32x4_t pairwise_min_s32(int32x4_t a, int32x4_t b) { return vpminq_s32(a, b); }
