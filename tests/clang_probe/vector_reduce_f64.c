#include <arm_neon.h>

double add2f_reduce(float64x2_t a) { return vaddvq_f64(a); }
double max2f_reduce(float64x2_t a) { return vmaxvq_f64(a); }
double min2f_reduce(float64x2_t a) { return vminvq_f64(a); }
