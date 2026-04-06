/* Workbench note: this probe exposed the byte-lane integer vector pairwise
 * extrema family from Apple clang, which led to adding `umaxp.16b`,
 * `uminp.16b`, `smaxp.16b`, and `sminp.16b` and promoting the case into
 * `tests/clang_probe/vector_pairwise_extrema_b.c`.
 */
#include <arm_neon.h>

uint8x16_t pairwise_max_u8(uint8x16_t a, uint8x16_t b) { return vpmaxq_u8(a, b); }
uint8x16_t pairwise_min_u8(uint8x16_t a, uint8x16_t b) { return vpminq_u8(a, b); }
int8x16_t pairwise_max_s8(int8x16_t a, int8x16_t b) { return vpmaxq_s8(a, b); }
int8x16_t pairwise_min_s8(int8x16_t a, int8x16_t b) { return vpminq_s8(a, b); }
