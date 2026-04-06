/* Workbench note: probe integer lane extract/insert shapes from Apple clang. */
#include <arm_neon.h>
unsigned get_lane_u32(uint32x4_t a) { return vgetq_lane_u32(a, 2); }
unsigned short get_lane_u16(uint16x8_t a) { return vgetq_lane_u16(a, 5); }
unsigned char get_lane_u8(uint8x16_t a) { return vgetq_lane_u8(a, 7); }
uint32x4_t set_lane_u32(uint32x4_t a, unsigned x) { return vsetq_lane_u32(x, a, 1); }
uint16x8_t set_lane_u16(uint16x8_t a, unsigned short x) { return vsetq_lane_u16(x, a, 5); }
uint8x16_t set_lane_u8(uint8x16_t a, unsigned char x) { return vsetq_lane_u8(x, a, 7); }
