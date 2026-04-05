#include <arm_neon.h>

float32x4_t round_away(float32x4_t a) { return vrndaq_f32(a); }
float32x4_t round_current(float32x4_t a) { return vrndiq_f32(a); }
