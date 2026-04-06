#include <arm_neon.h>

float64x2_t recip_est2(float64x2_t a) { return vrecpeq_f64(a); }
float64x2_t recip_step2(float64x2_t a, float64x2_t b) { return vrecpsq_f64(a, b); }
float64x2_t rsqrt_est2(float64x2_t a) { return vrsqrteq_f64(a); }
float64x2_t rsqrt_step2(float64x2_t a, float64x2_t b) { return vrsqrtsq_f64(a, b); }
