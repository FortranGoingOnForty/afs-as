#include <arm_neon.h>

float32x4_t round_nearest(float32x4_t a) { return vrndnq_f32(a); }
float32x4_t round_down(float32x4_t a) { return vrndmq_f32(a); }
float32x4_t round_up(float32x4_t a) { return vrndpq_f32(a); }
float32x4_t round_zero(float32x4_t a) { return vrndq_f32(a); }
