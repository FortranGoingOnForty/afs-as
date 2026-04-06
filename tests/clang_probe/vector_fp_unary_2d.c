#include <arm_neon.h>

float64x2_t abs2f(float64x2_t a) { return vabsq_f64(a); }
float64x2_t neg2f(float64x2_t a) { return vnegq_f64(a); }
float64x2_t sqrt2f(float64x2_t a) { return vsqrtq_f64(a); }
