#include <arm_neon.h>
#include <math.h>

double maxnm2f_reduce(float64x2_t a) { return vmaxnmvq_f64(a); }
double minnm2f_reduce(float64x2_t a) { return vminnmvq_f64(a); }
