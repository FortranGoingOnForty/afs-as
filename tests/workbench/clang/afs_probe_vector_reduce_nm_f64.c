/* Workbench note: probe float64 NaN-aware horizontal reductions to see whether
 * clang emits scalar-result `fmaxnmp.2d dN, vM` / `fminnmp.2d dN, vM`.
 */
#include <arm_neon.h>

double maxnm2f_reduce(float64x2_t a) { return vmaxnmvq_f64(a); }
double minnm2f_reduce(float64x2_t a) { return vminnmvq_f64(a); }
