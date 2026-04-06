#include <arm_neon.h>

float64x2_t round_nearest2(float64x2_t a) { return vrndnq_f64(a); }
float64x2_t round_down2(float64x2_t a) { return vrndmq_f64(a); }
float64x2_t round_up2(float64x2_t a) { return vrndpq_f64(a); }
float64x2_t round_zero2(float64x2_t a) { return vrndq_f64(a); }
float64x2_t round_away2(float64x2_t a) { return vrndaq_f64(a); }
float64x2_t round_current2(float64x2_t a) { return vrndiq_f64(a); }
