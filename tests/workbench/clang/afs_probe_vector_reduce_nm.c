// Workbench note: this probe showed that Apple clang emits a clean NaN-aware
// floating-point vector reduction family after `fmaxnm.4s` / `fminnm.4s`,
// which led directly to landing `fmaxnmv.4s` and `fminnmv.4s`.
#include <arm_neon.h>

float maxnm4f_reduce(float32x4_t v) { return vmaxnmvq_f32(v); }
float minnm4f_reduce(float32x4_t v) { return vminnmvq_f32(v); }
