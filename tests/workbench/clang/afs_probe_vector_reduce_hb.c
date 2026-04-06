/* Workbench note: probe the narrower integer reduction neighborhood after the
 * existing .4s reduction coverage to see whether clang exposes addv/smaxv/
 * sminv/umaxv/uminv on .8h or .16b lane shapes.
 */
#include <arm_neon.h>

uint16_t add_u8_reduce(uint8x16_t a) { return vaddvq_u8(a); }
int16_t add_s8_reduce(int8x16_t a) { return vaddvq_s8(a); }
uint16_t add_u16_reduce(uint16x8_t a) { return vaddvq_u16(a); }
int16_t add_s16_reduce(int16x8_t a) { return vaddvq_s16(a); }
uint8_t max_u8_reduce(uint8x16_t a) { return vmaxvq_u8(a); }
int8_t max_s8_reduce(int8x16_t a) { return vmaxvq_s8(a); }
uint16_t max_u16_reduce(uint16x8_t a) { return vmaxvq_u16(a); }
int16_t max_s16_reduce(int16x8_t a) { return vmaxvq_s16(a); }
uint8_t min_u8_reduce(uint8x16_t a) { return vminvq_u8(a); }
int8_t min_s8_reduce(int8x16_t a) { return vminvq_s8(a); }
uint16_t min_u16_reduce(uint16x8_t a) { return vminvq_u16(a); }
int16_t min_s16_reduce(int16x8_t a) { return vminvq_s16(a); }
