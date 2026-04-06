/* Workbench note: probe practical reduction-style Neon output after the
 * min/max families, to see whether Apple clang emits a clean addv/smaxv/umaxv
 * neighborhood that is worth landing as the next SIMD slice.
 */
#include <arm_neon.h>

unsigned sum4u(uint32x4_t a) { return vaddvq_u32(a); }
unsigned max4u(uint32x4_t a) { return vmaxvq_u32(a); }
int max4s(int32x4_t a) { return vmaxvq_s32(a); }
