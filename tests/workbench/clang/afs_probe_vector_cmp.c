/* Workbench note: probe vector compare/select shapes from Apple clang to pick the next SIMD family after lane crossover support. */
typedef unsigned v4u __attribute__((vector_size(16)));
typedef int v4i __attribute__((vector_size(16)));

v4u eq_mask_u32(v4u a, v4u b) { return a == b; }

v4i gt_mask_s32(v4i a, v4i b) { return a > b; }

v4u select_eq_u32(v4u a, v4u b) {
    v4u mask = a == b;
    return (mask & a) | (~mask & b);
}
