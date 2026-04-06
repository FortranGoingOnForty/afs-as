/* Workbench note: probe the next SIMD compare family after cmeq/cmgt to see whether clang asks for signed/unsigned ge/gt masks like cmge/cmhi/cmhs. */
typedef unsigned v4u __attribute__((vector_size(16)));
typedef int v4i __attribute__((vector_size(16)));

v4u ge_mask_u32(v4u a, v4u b) { return a >= b; }

v4u gt_mask_u32(v4u a, v4u b) { return a > b; }

v4i ge_mask_s32(v4i a, v4i b) { return a >= b; }
