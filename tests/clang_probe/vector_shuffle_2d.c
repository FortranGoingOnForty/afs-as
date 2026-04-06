#include <arm_neon.h>

float64x2_t zip1_2d(float64x2_t a, float64x2_t b) { return vzip1q_f64(a, b); }
float64x2_t zip2_2d(float64x2_t a, float64x2_t b) { return vzip2q_f64(a, b); }
float64x2_t uzp1_2d(float64x2_t a, float64x2_t b) { return vuzp1q_f64(a, b); }
float64x2_t uzp2_2d(float64x2_t a, float64x2_t b) { return vuzp2q_f64(a, b); }
float64x2_t trn1_2d(float64x2_t a, float64x2_t b) { return vtrn1q_f64(a, b); }
float64x2_t trn2_2d(float64x2_t a, float64x2_t b) { return vtrn2q_f64(a, b); }
