/* Workbench note: this probe exposed the practical integer vector pairwise
 * add family from Apple clang, which led to adding `addp.4s`
 * and promoting the case into `tests/clang_probe/vector_pairwise_int.c`.
 */
#include <arm_neon.h>

uint32x4_t pairwise_add_u32(uint32x4_t a, uint32x4_t b) { return vpaddq_u32(a, b); }
int32x4_t pairwise_add_s32(int32x4_t a, int32x4_t b) { return vpaddq_s32(a, b); }
