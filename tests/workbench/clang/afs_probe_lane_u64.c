/* Workbench note: probe 64-bit integer lane extract/insert shapes from Apple clang. */
#include <arm_neon.h>
unsigned long long get_lane_u64(uint64x2_t a) { return vgetq_lane_u64(a, 1); }
uint64x2_t set_lane_u64(uint64x2_t a, unsigned long long x) { return vsetq_lane_u64(x, a, 1); }
