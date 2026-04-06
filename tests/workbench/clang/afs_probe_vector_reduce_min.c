/* Workbench note: probe the min-reduction siblings after landing addv/umaxv/
 * smaxv, to see whether Apple clang emits a clean uminv/sminv family from the
 * matching Neon intrinsics.
 */
#include <arm_neon.h>

unsigned min4u_reduce(uint32x4_t a) { return vminvq_u32(a); }
int min4s_reduce(int32x4_t a) { return vminvq_s32(a); }
