#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

struct ProbeCase {
    name: &'static str,
    source: &'static str,
    driver: &'static str,
    support: Option<&'static str>,
}

#[derive(Clone, Copy, Default)]
struct StageStatus {
    parse: bool,
    assemble: bool,
    semantics: bool,
    raw: bool,
    link: bool,
    run: bool,
}

struct ProbeResult {
    case: &'static str,
    opt: &'static str,
    status: StageStatus,
    detail: Option<String>,
}

const CASES: &[ProbeCase] = &[
    ProbeCase {
        name: "simple_math",
        source: "simple_math.c",
        driver: "extern int square_plus_three(int);\nint main(void) { return square_plus_three(4) != 19; }\n",
        support: None,
    },
    ProbeCase {
        name: "globals",
        source: "globals.c",
        driver: "extern int read_global_plus_one(void);\nint main(void) { return read_global_plus_one() != 8; }\n",
        support: None,
    },
    ProbeCase {
        name: "struct_global",
        source: "struct_global.c",
        driver: "extern int sum_pair(void);\nint main(void) { return sum_pair() != 3; }\n",
        support: None,
    },
    ProbeCase {
        name: "switch_stmt",
        source: "switch_stmt.c",
        driver: "extern int classify(int);\nint main(void) {\n    return (classify(0) != 10) || (classify(1) != 20) || (classify(2) != 30) || (classify(9) != 40);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "stack_frame",
        source: "stack_frame.c",
        driver: "extern int frame_heavy(int, int, int, int);\nint main(void) { return frame_heavy(5, 6, 7, 8) != 9; }\n",
        support: None,
    },
    ProbeCase {
        name: "counted_loop",
        source: "counted_loop.c",
        driver: "extern int sum_to_n(int);\nint main(void) { return sum_to_n(10) != 45; }\n",
        support: None,
    },
    ProbeCase {
        name: "div_mod",
        source: "div_mod.c",
        driver: "extern int div_mod_mix(int, int);\nint main(void) { return div_mod_mix(17, 5) != 5; }\n",
        support: None,
    },
    ProbeCase {
        name: "ext_global",
        source: "ext_global.c",
        driver: "extern int read_ext_plus_one(void);\nint main(void) { return read_ext_plus_one() != 42; }\n",
        support: Some("int ext_value = 41;\n"),
    },
    ProbeCase {
        name: "ext_array",
        source: "ext_array.c",
        driver: "extern int read_ext_array_slot(int);\nint main(void) {\n    return (read_ext_array_slot(0) != 11)\n        || (read_ext_array_slot(1) != 22)\n        || (read_ext_array_slot(2) != 33)\n        || (read_ext_array_slot(7) != 44);\n}\n",
        support: Some("int ext_array[4] = {11, 22, 33, 44};\n"),
    },
    ProbeCase {
        name: "ext_struct_field",
        source: "ext_struct_field.c",
        driver: "extern int read_ext_pair_b(void);\nint main(void) { return read_ext_pair_b() != 17; }\n",
        support: Some("struct Pair { int a; int b; };\nstruct Pair ext_pair = {3, 17};\n"),
    },
    ProbeCase {
        name: "ext_ptr_deref",
        source: "ext_ptr_deref.c",
        driver: "extern int read_ext_ptr(void);\nint main(void) { return read_ext_ptr() != 29; }\n",
        support: Some("int ext_value = 29;\nint *ext_ptr = &ext_value;\n"),
    },
    ProbeCase {
        name: "ext_byte_ptr",
        source: "ext_byte_ptr.c",
        driver: "extern int read_ext_byte3(void);\nint main(void) { return read_ext_byte3() != 77; }\n",
        support: Some("unsigned char ext_storage[] = {1, 2, 3, 77, 5};\nunsigned char *ext_bytes = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_signed_byte_ptr",
        source: "ext_signed_byte_ptr.c",
        driver: "extern int read_ext_sbyte3(void);\nint main(void) { return read_ext_sbyte3() != -11; }\n",
        support: Some("signed char ext_storage[] = {1, 2, 3, -11, 5};\nsigned char *ext_sbytes = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_short_ptr",
        source: "ext_short_ptr.c",
        driver: "extern int read_ext_short2(void);\nint main(void) { return read_ext_short2() != 321; }\n",
        support: Some("unsigned short ext_storage[] = {7, 9, 321, 11};\nunsigned short *ext_shorts = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_signed_short_ptr",
        source: "ext_signed_short_ptr.c",
        driver: "extern int read_ext_short_signed2(void);\nint main(void) { return read_ext_short_signed2() != -321; }\n",
        support: Some("short ext_storage[] = {7, 9, -321, 11};\nshort *ext_shorts_signed = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_str_index",
        source: "ext_str_index.c",
        driver: "extern int second_ext_char(void);\nint main(void) { return second_ext_char() != 'Q'; }\n",
        support: Some("const char *ext_str = \"zQ\";\n"),
    },
    ProbeCase {
        name: "ext_ptr_to_struct",
        source: "ext_ptr_to_struct.c",
        driver: "extern int read_ext_pair_ptr_b(void);\nint main(void) { return read_ext_pair_ptr_b() != 19; }\n",
        support: Some("struct Pair { int a; int b; };\nstatic struct Pair pair = {8, 19};\nstruct Pair *ext_pair_ptr = &pair;\n"),
    },
    ProbeCase {
        name: "func_ptr",
        source: "func_ptr.c",
        driver: "extern int call_helper_ptr(int);\nint main(void) {\n    return (call_helper_ptr(4) != 15) || (call_helper_ptr(-1) != 0);\n}\n",
        support: Some("int helper(int x) { return x * 3; }\n"),
    },
    ProbeCase {
        name: "func_slot_array",
        source: "func_slot_array.c",
        driver: "extern int call_helper_slot1(int);\nint main(void) {\n    return (call_helper_slot1(2) != 35) || (call_helper_slot1(-3) != 0);\n}\n",
        support: Some("int helper_a(int x) { return x + 1; }\nint helper_b(int x) { return x * 7; }\nint (*helper_slots[2])(int) = {helper_a, helper_b};\n"),
    },
    ProbeCase {
        name: "func_slot",
        source: "func_slot.c",
        driver: "extern int call_helper_slot(int);\nint main(void) {\n    return (call_helper_slot(4) != 30) || (call_helper_slot(-2) != 0);\n}\n",
        support: Some("int helper_impl(int x) { return x * 5; }\nint (*helper_slot)(int) = helper_impl;\n"),
    },
    ProbeCase {
        name: "float_branch",
        source: "float_branch.c",
        driver: "extern double clampish(double, double);\nint main(void) { double got = clampish(2.0, 3.0); return (got < 9.49) || (got > 9.51); }\n",
        support: None,
    },
    ProbeCase {
        name: "vector_add",
        source: "vector_add.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f add4(v4f, v4f);\nextern void add4_store(float *, const float *, const float *);\nint main(void) {\n    v4f a = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f b = {10.0f, 20.0f, 30.0f, 40.0f};\n    v4f c = add4(a, b);\n    float out[4] = {0.0f, 0.0f, 0.0f, 0.0f};\n    add4_store(out, (const float *)&a, (const float *)&b);\n    return (c[0] != 11.0f) || (c[1] != 22.0f) || (c[2] != 33.0f) || (c[3] != 44.0f)\n        || (out[0] != 11.0f) || (out[1] != 22.0f) || (out[2] != 33.0f) || (out[3] != 44.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_math",
        source: "vector_math.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f sub4(v4f, v4f);\nextern v4f mul4(v4f, v4f);\nextern v4f div4(v4f, v4f);\nint main(void) {\n    v4f a = {8.0f, 6.0f, 4.0f, 2.0f};\n    v4f b = {1.0f, 2.0f, 4.0f, 8.0f};\n    v4f sub = sub4(a, b);\n    v4f mul = mul4(a, b);\n    v4f div = div4(a, b);\n    return (sub[0] != 7.0f) || (sub[1] != 4.0f) || (sub[2] != 0.0f) || (sub[3] != -6.0f)\n        || (mul[0] != 8.0f) || (mul[1] != 12.0f) || (mul[2] != 16.0f) || (mul[3] != 16.0f)\n        || (div[0] != 8.0f) || (div[1] != 3.0f) || (div[2] != 1.0f) || (div[3] != 0.25f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pair",
        source: "vector_pair.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern void use_pair(const v4f *, v4f *);\nint main(void) {\n    v4f in[2] = {{1.0f, 2.0f, 3.0f, 4.0f}, {10.0f, 20.0f, 30.0f, 40.0f}};\n    v4f out[2] = {{0.0f, 0.0f, 0.0f, 0.0f}, {0.0f, 0.0f, 0.0f, 0.0f}};\n    float *lo = (float *)&out[0];\n    float *hi = (float *)&out[1];\n    use_pair(in, out);\n    return (lo[0] != 11.0f) || (lo[1] != 22.0f) || (lo[2] != 33.0f) || (lo[3] != 44.0f)\n        || (hi[0] != -9.0f) || (hi[1] != -18.0f) || (hi[2] != -27.0f) || (hi[3] != -36.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pair_return",
        source: "vector_pair_return.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\ntypedef struct { v4f a; v4f b; } pair;\nextern pair make_pair(v4f, v4f);\nint main(void) {\n    v4f x = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f y = {10.0f, 20.0f, 30.0f, 40.0f};\n    pair p = make_pair(x, y);\n    float *lo = (float *)&p.a;\n    float *hi = (float *)&p.b;\n    return (lo[0] != 11.0f) || (lo[1] != 22.0f) || (lo[2] != 33.0f) || (lo[3] != 44.0f)\n        || (hi[0] != -9.0f) || (hi[1] != -18.0f) || (hi[2] != -27.0f) || (hi[3] != -36.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_lane_transfer",
        source: "vector_lane_transfer.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\ntypedef double v2d __attribute__((vector_size(16)));\nextern float lane2f(v4f);\nextern v4f set_lane0f(v4f, float);\nextern double lane1d(v2d);\nextern v2d set_lane1d(v2d, double);\nint main(void) {\n    v4f xf = {1.0f, 2.0f, 3.0f, 4.0f};\n    v2d xd = {1.5, 2.5};\n    v4f yf = set_lane0f(xf, 9.0f);\n    v2d yd = set_lane1d(xd, 7.5);\n    return (lane2f(xf) != 3.0f)\n        || (yf[0] != 9.0f) || (yf[1] != 2.0f) || (yf[2] != 3.0f) || (yf[3] != 4.0f)\n        || (lane1d(xd) != 2.5)\n        || (yd[0] != 1.5) || (yd[1] != 7.5);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_lane_gp",
        source: "vector_lane_gp.c",
        driver: "typedef unsigned v4u __attribute__((vector_size(16)));\ntypedef unsigned short v8hu __attribute__((vector_size(16)));\ntypedef unsigned char v16bu __attribute__((vector_size(16)));\ntypedef unsigned long long v2ull __attribute__((vector_size(16)));\nextern unsigned get_lane_u32(v4u);\nextern unsigned short get_lane_u16(v8hu);\nextern unsigned char get_lane_u8(v16bu);\nextern unsigned long long get_lane_u64(v2ull);\nextern v4u set_lane_u32(v4u, unsigned);\nextern v8hu set_lane_u16(v8hu, unsigned short);\nextern v16bu set_lane_u8(v16bu, unsigned char);\nextern v2ull set_lane_u64(v2ull, unsigned long long);\nint main(void) {\n    v4u a = {10u, 20u, 30u, 40u};\n    v8hu h = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u};\n    v16bu b = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u};\n    v2ull d = {11ull, 22ull};\n    v4u ra = set_lane_u32(a, 99u);\n    v8hu rh = set_lane_u16(h, 55u);\n    v16bu rb = set_lane_u8(b, 77u);\n    v2ull rd = set_lane_u64(d, 88ull);\n    return (get_lane_u32(a) != 30u)\n        || (get_lane_u16(h) != 6u)\n        || (get_lane_u8(b) != 8u)\n        || (get_lane_u64(d) != 22ull)\n        || (ra[0] != 10u) || (ra[1] != 99u) || (ra[2] != 30u) || (ra[3] != 40u)\n        || (rh[0] != 1u) || (rh[5] != 55u) || (rh[7] != 8u)\n        || (rb[0] != 1u) || (rb[7] != 77u) || (rb[15] != 16u)\n        || (rd[0] != 11ull) || (rd[1] != 88ull);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_int_ops",
        source: "vector_int_ops.c",
        driver: "typedef int v4i __attribute__((vector_size(16)));\ntypedef unsigned char v16u8 __attribute__((vector_size(16)));\nextern v4i add4i(v4i, v4i);\nextern v4i sub4i(v4i, v4i);\nextern v16u8 and16(v16u8, v16u8);\nextern v16u8 or16(v16u8, v16u8);\nextern v16u8 xor16(v16u8, v16u8);\nint main(void) {\n    v4i a = {10, 20, 30, 40};\n    v4i b = {1, 2, 3, 4};\n    v16u8 x = {0xFF, 0x0F, 0xF0, 0x55, 0xAA, 0x12, 0x34, 0x56, 0x80, 0x7F, 0x33, 0xCC, 0x5A, 0xA5, 0x11, 0x22};\n    v16u8 y = {0x0F, 0xF0, 0x0F, 0xAA, 0x55, 0x34, 0x12, 0x65, 0x7F, 0x80, 0xCC, 0x33, 0xA5, 0x5A, 0x22, 0x11};\n    v4i add = add4i(a, b);\n    v4i sub = sub4i(a, b);\n    v16u8 av = and16(x, y);\n    v16u8 ov = or16(x, y);\n    v16u8 xv = xor16(x, y);\n    return (add[0] != 11) || (add[1] != 22) || (add[2] != 33) || (add[3] != 44)\n        || (sub[0] != 9) || (sub[1] != 18) || (sub[2] != 27) || (sub[3] != 36)\n        || (av[0] != 0x0F) || (av[1] != 0x00) || (av[2] != 0x00) || (av[3] != 0x00)\n        || (ov[0] != 0xFF) || (ov[1] != 0xFF) || (ov[2] != 0xFF) || (ov[3] != 0xFF)\n        || (xv[4] != 0xFF) || (xv[5] != 0x26) || (xv[6] != 0x26) || (xv[7] != 0x33)\n        || (xv[8] != 0xFF) || (xv[9] != 0xFF) || (xv[10] != 0xFF) || (xv[11] != 0xFF)\n        || (xv[12] != 0xFF) || (xv[13] != 0xFF) || (xv[14] != 0x33) || (xv[15] != 0x33);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_cmp",
        source: "vector_cmp.c",
        driver: "typedef unsigned v4u __attribute__((vector_size(16)));\ntypedef int v4i __attribute__((vector_size(16)));\nextern v4u eq_mask_u32(v4u, v4u);\nextern v4i gt_mask_s32(v4i, v4i);\nextern v4u select_eq_u32(v4u, v4u);\nint main(void) {\n    v4u au = {1u, 2u, 3u, 4u};\n    v4u bu = {1u, 9u, 3u, 8u};\n    v4i as = {-1, 5, 7, -3};\n    v4i bs = {-2, 5, 6, 4};\n    v4u eq = eq_mask_u32(au, bu);\n    v4i gt = gt_mask_s32(as, bs);\n    v4u sel = select_eq_u32(au, bu);\n    return (eq[0] != 0xFFFFFFFFu) || (eq[1] != 0u) || (eq[2] != 0xFFFFFFFFu) || (eq[3] != 0u)\n        || ((unsigned)gt[0] != 0xFFFFFFFFu) || ((unsigned)gt[1] != 0u) || ((unsigned)gt[2] != 0xFFFFFFFFu) || ((unsigned)gt[3] != 0u)\n        || (sel[0] != 1u) || (sel[1] != 9u) || (sel[2] != 3u) || (sel[3] != 8u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_cmp_more",
        source: "vector_cmp_more.c",
        driver: "typedef unsigned v4u __attribute__((vector_size(16)));\ntypedef int v4i __attribute__((vector_size(16)));\nextern v4u ge_mask_u32(v4u, v4u);\nextern v4u gt_mask_u32(v4u, v4u);\nextern v4i ge_mask_s32(v4i, v4i);\nint main(void) {\n    v4u au = {5u, 2u, 7u, 4u};\n    v4u bu = {5u, 3u, 6u, 9u};\n    v4i as = {-1, 5, 7, -3};\n    v4i bs = {-2, 5, 6, 4};\n    v4u geu = ge_mask_u32(au, bu);\n    v4u gtu = gt_mask_u32(au, bu);\n    v4i ges = ge_mask_s32(as, bs);\n    return (geu[0] != 0xFFFFFFFFu) || (geu[1] != 0u) || (geu[2] != 0xFFFFFFFFu) || (geu[3] != 0u)\n        || (gtu[0] != 0u) || (gtu[1] != 0u) || (gtu[2] != 0xFFFFFFFFu) || (gtu[3] != 0u)\n        || ((unsigned)ges[0] != 0xFFFFFFFFu) || ((unsigned)ges[1] != 0xFFFFFFFFu) || ((unsigned)ges[2] != 0xFFFFFFFFu) || ((unsigned)ges[3] != 0u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_minmax",
        source: "vector_minmax.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t max4f(float32x4_t, float32x4_t);\nextern float32x4_t min4f(float32x4_t, float32x4_t);\nextern int32x4_t max4s(int32x4_t, int32x4_t);\nextern int32x4_t min4s(int32x4_t, int32x4_t);\nint main(void) {\n    float32x4_t af = {1.0f, 9.0f, -3.0f, 4.0f};\n    float32x4_t bf = {2.0f, 7.0f, -5.0f, 8.0f};\n    int32x4_t ai = {1, 9, -3, 4};\n    int32x4_t bi = {2, 7, -5, 8};\n    float32x4_t xf = max4f(af, bf);\n    float32x4_t nf = min4f(af, bf);\n    int32x4_t xi = max4s(ai, bi);\n    int32x4_t ni = min4s(ai, bi);\n    return (xf[0] != 2.0f) || (xf[1] != 9.0f) || (xf[2] != -3.0f) || (xf[3] != 8.0f)\n        || (nf[0] != 1.0f) || (nf[1] != 7.0f) || (nf[2] != -5.0f) || (nf[3] != 4.0f)\n        || (xi[0] != 2) || (xi[1] != 9) || (xi[2] != -3) || (xi[3] != 8)\n        || (ni[0] != 1) || (ni[1] != 7) || (ni[2] != -5) || (ni[3] != 4);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_minmax_unsigned",
        source: "vector_minmax_unsigned.c",
        driver: "#include <arm_neon.h>\nextern uint32x4_t max4u(uint32x4_t, uint32x4_t);\nextern uint32x4_t min4u(uint32x4_t, uint32x4_t);\nint main(void) {\n    uint32x4_t a = {1u, 9u, 3u, 4u};\n    uint32x4_t b = {2u, 7u, 5u, 8u};\n    uint32x4_t x = max4u(a, b);\n    uint32x4_t n = min4u(a, b);\n    return (x[0] != 2u) || (x[1] != 9u) || (x[2] != 5u) || (x[3] != 8u)\n        || (n[0] != 1u) || (n[1] != 7u) || (n[2] != 3u) || (n[3] != 4u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_minmax_nm",
        source: "vector_minmax_nm.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t maxnm4f(float32x4_t, float32x4_t);\nextern float32x4_t minnm4f(float32x4_t, float32x4_t);\nint main(void) {\n    float32x4_t a = {1.0f, 9.0f, -3.0f, 4.0f};\n    float32x4_t b = {2.0f, 7.0f, -5.0f, 8.0f};\n    float32x4_t x = maxnm4f(a, b);\n    float32x4_t n = minnm4f(a, b);\n    return (x[0] != 2.0f) || (x[1] != 9.0f) || (x[2] != -3.0f) || (x[3] != 8.0f)\n        || (n[0] != 1.0f) || (n[1] != 7.0f) || (n[2] != -5.0f) || (n[3] != 4.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_minmax_nm_2d",
        source: "vector_minmax_nm_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t maxnm2d(float64x2_t, float64x2_t);\nextern float64x2_t minnm2d(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t a = {1.0, 9.0};\n    float64x2_t b = {2.0, 7.0};\n    float64x2_t x = maxnm2d(a, b);\n    float64x2_t n = minnm2d(a, b);\n    return (x[0] != 2.0) || (x[1] != 9.0)\n        || (n[0] != 1.0) || (n[1] != 7.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce",
        source: "vector_reduce.c",
        driver: "#include <arm_neon.h>\nextern unsigned sum4u(uint32x4_t);\nextern unsigned max4u_reduce(uint32x4_t);\nextern int max4s_reduce(int32x4_t);\nint main(void) {\n    uint32x4_t au = {1u, 9u, 3u, 4u};\n    int32x4_t as = {-1, 5, 7, -3};\n    return (sum4u(au) != 17u) || (max4u_reduce(au) != 9u) || (max4s_reduce(as) != 7);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_min",
        source: "vector_reduce_min.c",
        driver: "#include <arm_neon.h>\nextern unsigned min4u_reduce(uint32x4_t);\nextern int min4s_reduce(int32x4_t);\nint main(void) {\n    uint32x4_t au = {1u, 9u, 3u, 4u};\n    int32x4_t as = {-1, 5, 7, -3};\n    return (min4u_reduce(au) != 1u) || (min4s_reduce(as) != -3);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_hb",
        source: "vector_reduce_hb.c",
        driver: "#include <arm_neon.h>\nextern uint16_t add_u8_reduce(uint8x16_t);\nextern int16_t add_s8_reduce(int8x16_t);\nextern uint16_t add_u16_reduce(uint16x8_t);\nextern int16_t add_s16_reduce(int16x8_t);\nextern uint8_t max_u8_reduce(uint8x16_t);\nextern int8_t max_s8_reduce(int8x16_t);\nextern uint16_t max_u16_reduce(uint16x8_t);\nextern int16_t max_s16_reduce(int16x8_t);\nextern uint8_t min_u8_reduce(uint8x16_t);\nextern int8_t min_s8_reduce(int8x16_t);\nextern uint16_t min_u16_reduce(uint16x8_t);\nextern int16_t min_s16_reduce(int16x8_t);\nint main(void) {\n    uint8x16_t u8 = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u};\n    int8x16_t s8 = {1, -2, 3, -4, 5, -6, 7, -8, 9, -10, 11, -12, 13, -14, 15, -16};\n    uint16x8_t u16 = {10u, 20u, 30u, 40u, 50u, 60u, 70u, 80u};\n    int16x8_t s16 = {10, -20, 30, -40, 50, -60, 70, -80};\n    return (add_u8_reduce(u8) != 136u) || (add_s8_reduce(s8) != -8)\n        || (add_u16_reduce(u16) != 360u) || (add_s16_reduce(s16) != -40)\n        || (max_u8_reduce(u8) != 16u) || (max_s8_reduce(s8) != 15)\n        || (max_u16_reduce(u16) != 80u) || (max_s16_reduce(s16) != 70)\n        || (min_u8_reduce(u8) != 1u) || (min_s8_reduce(s8) != -16)\n        || (min_u16_reduce(u16) != 10u) || (min_s16_reduce(s16) != -80);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_fp",
        source: "vector_reduce_fp.c",
        driver: "#include <arm_neon.h>\nextern float add4f_reduce(float32x4_t);\nextern float max4f_reduce(float32x4_t);\nextern float min4f_reduce(float32x4_t);\nint main(void) {\n    float32x4_t a = {1.0f, 9.0f, -3.0f, 4.0f};\n    return (add4f_reduce(a) != 11.0f) || (max4f_reduce(a) != 9.0f) || (min4f_reduce(a) != -3.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_f64",
        source: "vector_reduce_f64.c",
        driver: "#include <arm_neon.h>\nextern double add2f_reduce(float64x2_t);\nextern double max2f_reduce(float64x2_t);\nextern double min2f_reduce(float64x2_t);\nint main(void) {\n    float64x2_t a = {1.5, 9.0};\n    return (add2f_reduce(a) != 10.5) || (max2f_reduce(a) != 9.0) || (min2f_reduce(a) != 1.5);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_nm_f64",
        source: "vector_reduce_nm_f64.c",
        driver: "#include <arm_neon.h>\n#include <math.h>\nextern double maxnm2f_reduce(float64x2_t);\nextern double minnm2f_reduce(float64x2_t);\nint main(void) {\n    float64x2_t a = {NAN, 9.0};\n    return (maxnm2f_reduce(a) != 9.0) || (minnm2f_reduce(a) != 9.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_fp",
        source: "vector_pairwise_fp.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t pairwise_max(float32x4_t, float32x4_t);\nextern float32x4_t pairwise_min(float32x4_t, float32x4_t);\nint main(void) {\n    float32x4_t a = {1.0f, 9.0f, -3.0f, 4.0f};\n    float32x4_t b = {2.0f, 7.0f, -5.0f, 8.0f};\n    float32x4_t x = pairwise_max(a, b);\n    float32x4_t y = pairwise_min(a, b);\n    return (x[0] != 9.0f) || (x[1] != 4.0f) || (x[2] != 7.0f) || (x[3] != 8.0f)\n        || (y[0] != 1.0f) || (y[1] != -3.0f) || (y[2] != 2.0f) || (y[3] != -5.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_nm",
        source: "vector_pairwise_nm.c",
        driver: "#include <arm_neon.h>\n#include <math.h>\nextern float32x4_t pairwise_maxnm(float32x4_t, float32x4_t);\nextern float32x4_t pairwise_minnm(float32x4_t, float32x4_t);\nint main(void) {\n    float32x4_t a = {NAN, 9.0f, -3.0f, 4.0f};\n    float32x4_t b = {2.0f, NAN, -5.0f, 8.0f};\n    float32x4_t x = pairwise_maxnm(a, b);\n    float32x4_t y = pairwise_minnm(a, b);\n    return (x[0] != 9.0f) || (x[1] != 4.0f) || (x[2] != 2.0f) || (x[3] != 8.0f)\n        || (y[0] != 9.0f) || (y[1] != -3.0f) || (y[2] != 2.0f) || (y[3] != -5.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_int",
        source: "vector_pairwise_int.c",
        driver: "#include <arm_neon.h>\nextern uint32x4_t pairwise_add_u32(uint32x4_t, uint32x4_t);\nextern int32x4_t pairwise_add_s32(int32x4_t, int32x4_t);\nint main(void) {\n    uint32x4_t au = {1u, 9u, 3u, 4u};\n    uint32x4_t bu = {2u, 7u, 5u, 8u};\n    int32x4_t as = {1, 9, -3, 4};\n    int32x4_t bs = {2, 7, -5, 8};\n    uint32x4_t xu = pairwise_add_u32(au, bu);\n    int32x4_t xs = pairwise_add_s32(as, bs);\n    return (xu[0] != 10u) || (xu[1] != 7u) || (xu[2] != 9u) || (xu[3] != 13u)\n        || (xs[0] != 10) || (xs[1] != 1) || (xs[2] != 9) || (xs[3] != 3);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_add_hb",
        source: "vector_pairwise_add_hb.c",
        driver: "#include <arm_neon.h>\nextern uint16x8_t pairwise_add_u16(uint16x8_t, uint16x8_t);\nextern int16x8_t pairwise_add_s16(int16x8_t, int16x8_t);\nextern uint8x16_t pairwise_add_u8(uint8x16_t, uint8x16_t);\nextern int8x16_t pairwise_add_s8(int8x16_t, int8x16_t);\nint main(void) {\n    uint16x8_t au16 = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u};\n    uint16x8_t bu16 = {9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u};\n    int16x8_t as16 = {1, -2, 3, -4, 5, -6, 7, -8};\n    int16x8_t bs16 = {9, -10, 11, -12, 13, -14, 15, -16};\n    uint8x16_t au8 = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u};\n    uint8x16_t bu8 = {17u, 18u, 19u, 20u, 21u, 22u, 23u, 24u, 25u, 26u, 27u, 28u, 29u, 30u, 31u, 32u};\n    int8x16_t as8 = {1, -2, 3, -4, 5, -6, 7, -8, 9, -10, 11, -12, 13, -14, 15, -16};\n    int8x16_t bs8 = {17, -18, 19, -20, 21, -22, 23, -24, 25, -26, 27, -28, 29, -30, 31, -32};\n    uint16x8_t xu16 = pairwise_add_u16(au16, bu16);\n    int16x8_t xs16 = pairwise_add_s16(as16, bs16);\n    uint8x16_t xu8 = pairwise_add_u8(au8, bu8);\n    int8x16_t xs8 = pairwise_add_s8(as8, bs8);\n    return (xu16[0] != 3u) || (xu16[1] != 7u) || (xu16[2] != 11u) || (xu16[3] != 15u)\n        || (xu16[4] != 19u) || (xu16[5] != 23u) || (xu16[6] != 27u) || (xu16[7] != 31u)\n        || (xs16[0] != -1) || (xs16[1] != -1) || (xs16[2] != -1) || (xs16[3] != -1)\n        || (xs16[4] != -1) || (xs16[5] != -1) || (xs16[6] != -1) || (xs16[7] != -1)\n        || (xu8[0] != 3u) || (xu8[1] != 7u) || (xu8[2] != 11u) || (xu8[3] != 15u)\n        || (xu8[4] != 19u) || (xu8[5] != 23u) || (xu8[6] != 27u) || (xu8[7] != 31u)\n        || (xu8[8] != 35u) || (xu8[9] != 39u) || (xu8[10] != 43u) || (xu8[11] != 47u)\n        || (xu8[12] != 51u) || (xu8[13] != 55u) || (xu8[14] != 59u) || (xu8[15] != 63u)\n        || (xs8[0] != -1) || (xs8[1] != -1) || (xs8[2] != -1) || (xs8[3] != -1)\n        || (xs8[4] != -1) || (xs8[5] != -1) || (xs8[6] != -1) || (xs8[7] != -1)\n        || (xs8[8] != -1) || (xs8[9] != -1) || (xs8[10] != -1) || (xs8[11] != -1)\n        || (xs8[12] != -1) || (xs8[13] != -1) || (xs8[14] != -1) || (xs8[15] != -1);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_2d",
        source: "vector_pairwise_2d.c",
        driver: "#include <arm_neon.h>\nextern uint64x2_t pairwise_add_u64(uint64x2_t, uint64x2_t);\nextern int64x2_t pairwise_add_s64(int64x2_t, int64x2_t);\nextern float64x2_t pairwise_max_f64(float64x2_t, float64x2_t);\nextern float64x2_t pairwise_min_f64(float64x2_t, float64x2_t);\nint main(void) {\n    uint64x2_t au = {1ull, 9ull};\n    uint64x2_t bu = {3ull, 4ull};\n    int64x2_t as = {1ll, -9ll};\n    int64x2_t bs = {3ll, -4ll};\n    float64x2_t af = {1.5, 9.0};\n    float64x2_t bf = {3.25, -4.5};\n    uint64x2_t xu = pairwise_add_u64(au, bu);\n    int64x2_t xs = pairwise_add_s64(as, bs);\n    float64x2_t xf = pairwise_max_f64(af, bf);\n    float64x2_t nf = pairwise_min_f64(af, bf);\n    return (xu[0] != 10ull) || (xu[1] != 7ull)\n        || (xs[0] != -8ll) || (xs[1] != -1ll)\n        || (xf[0] != 9.0) || (xf[1] != 3.25)\n        || (nf[0] != 1.5) || (nf[1] != -4.5);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_nm_2d",
        source: "vector_pairwise_nm_2d.c",
        driver: "#include <arm_neon.h>\n#include <math.h>\nextern float64x2_t pairwise_maxnm_f64(float64x2_t, float64x2_t);\nextern float64x2_t pairwise_minnm_f64(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t a = {NAN, 9.0};\n    float64x2_t b = {2.0, -4.5};\n    float64x2_t x = pairwise_maxnm_f64(a, b);\n    float64x2_t y = pairwise_minnm_f64(a, b);\n    return (x[0] != 9.0) || (x[1] != 2.0)\n        || (y[0] != 9.0) || (y[1] != -4.5);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_add_fp_2d",
        source: "vector_pairwise_add_fp_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t pairwise_add_f64(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t a = {1.5, 9.0};\n    float64x2_t b = {3.25, -4.5};\n    float64x2_t x = pairwise_add_f64(a, b);\n    return (x[0] != 10.5) || (x[1] != -1.25);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_extrema",
        source: "vector_pairwise_extrema.c",
        driver: "#include <arm_neon.h>\nextern uint32x4_t pairwise_max_u32(uint32x4_t, uint32x4_t);\nextern uint32x4_t pairwise_min_u32(uint32x4_t, uint32x4_t);\nextern int32x4_t pairwise_max_s32(int32x4_t, int32x4_t);\nextern int32x4_t pairwise_min_s32(int32x4_t, int32x4_t);\nint main(void) {\n    uint32x4_t au = {1u, 9u, 3u, 4u};\n    uint32x4_t bu = {2u, 7u, 5u, 8u};\n    int32x4_t as = {1, 9, -3, 4};\n    int32x4_t bs = {2, 7, -5, 8};\n    uint32x4_t xu = pairwise_max_u32(au, bu);\n    uint32x4_t nu = pairwise_min_u32(au, bu);\n    int32x4_t xs = pairwise_max_s32(as, bs);\n    int32x4_t ns = pairwise_min_s32(as, bs);\n    return (xu[0] != 9u) || (xu[1] != 4u) || (xu[2] != 7u) || (xu[3] != 8u)\n        || (nu[0] != 1u) || (nu[1] != 3u) || (nu[2] != 2u) || (nu[3] != 5u)\n        || (xs[0] != 9) || (xs[1] != 4) || (xs[2] != 7) || (xs[3] != 8)\n        || (ns[0] != 1) || (ns[1] != -3) || (ns[2] != 2) || (ns[3] != -5);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_extrema_h",
        source: "vector_pairwise_extrema_h.c",
        driver: "#include <arm_neon.h>\nextern uint16x8_t pairwise_max_u16(uint16x8_t, uint16x8_t);\nextern uint16x8_t pairwise_min_u16(uint16x8_t, uint16x8_t);\nextern int16x8_t pairwise_max_s16(int16x8_t, int16x8_t);\nextern int16x8_t pairwise_min_s16(int16x8_t, int16x8_t);\nint main(void) {\n    uint16x8_t au = {1u, 9u, 3u, 4u, 10u, 0u, 6u, 2u};\n    uint16x8_t bu = {2u, 7u, 5u, 8u, 12u, 11u, 1u, 13u};\n    int16x8_t as = {1, 9, -3, 4, 10, -1, 6, -2};\n    int16x8_t bs = {2, 7, -5, 8, 12, 11, 1, -13};\n    uint16x8_t xu = pairwise_max_u16(au, bu);\n    uint16x8_t nu = pairwise_min_u16(au, bu);\n    int16x8_t xs = pairwise_max_s16(as, bs);\n    int16x8_t ns = pairwise_min_s16(as, bs);\n    return (xu[0] != 9u) || (xu[1] != 4u) || (xu[2] != 10u) || (xu[3] != 6u)\n        || (xu[4] != 7u) || (xu[5] != 8u) || (xu[6] != 12u) || (xu[7] != 13u)\n        || (nu[0] != 1u) || (nu[1] != 3u) || (nu[2] != 0u) || (nu[3] != 2u)\n        || (nu[4] != 2u) || (nu[5] != 5u) || (nu[6] != 11u) || (nu[7] != 1u)\n        || (xs[0] != 9) || (xs[1] != 4) || (xs[2] != 10) || (xs[3] != 6)\n        || (xs[4] != 7) || (xs[5] != 8) || (xs[6] != 12) || (xs[7] != 1)\n        || (ns[0] != 1) || (ns[1] != -3) || (ns[2] != -1) || (ns[3] != -2)\n        || (ns[4] != 2) || (ns[5] != -5) || (ns[6] != 11) || (ns[7] != -13);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_pairwise_extrema_b",
        source: "vector_pairwise_extrema_b.c",
        driver: "#include <arm_neon.h>\nextern uint8x16_t pairwise_max_u8(uint8x16_t, uint8x16_t);\nextern uint8x16_t pairwise_min_u8(uint8x16_t, uint8x16_t);\nextern int8x16_t pairwise_max_s8(int8x16_t, int8x16_t);\nextern int8x16_t pairwise_min_s8(int8x16_t, int8x16_t);\nint main(void) {\n    uint8x16_t au = {1u, 9u, 3u, 4u, 10u, 0u, 6u, 2u, 14u, 5u, 7u, 11u, 12u, 13u, 15u, 8u};\n    uint8x16_t bu = {2u, 7u, 5u, 8u, 12u, 11u, 1u, 13u, 4u, 16u, 3u, 10u, 9u, 6u, 0u, 17u};\n    int8x16_t as = {1, 9, -3, 4, 10, -1, 6, -2, 14, -5, 7, 11, 12, -13, 15, -8};\n    int8x16_t bs = {2, 7, -5, 8, 12, 11, 1, -13, 4, 16, 3, 10, 9, 6, 0, -17};\n    uint8x16_t xu = pairwise_max_u8(au, bu);\n    uint8x16_t nu = pairwise_min_u8(au, bu);\n    int8x16_t xs = pairwise_max_s8(as, bs);\n    int8x16_t ns = pairwise_min_s8(as, bs);\n    return (xu[0] != 9u) || (xu[1] != 4u) || (xu[2] != 10u) || (xu[3] != 6u)\n        || (xu[4] != 14u) || (xu[5] != 11u) || (xu[6] != 13u) || (xu[7] != 15u)\n        || (xu[8] != 7u) || (xu[9] != 8u) || (xu[10] != 12u) || (xu[11] != 13u)\n        || (xu[12] != 16u) || (xu[13] != 10u) || (xu[14] != 9u) || (xu[15] != 17u)\n        || (nu[0] != 1u) || (nu[1] != 3u) || (nu[2] != 0u) || (nu[3] != 2u)\n        || (nu[4] != 5u) || (nu[5] != 7u) || (nu[6] != 12u) || (nu[7] != 8u)\n        || (nu[8] != 2u) || (nu[9] != 5u) || (nu[10] != 11u) || (nu[11] != 1u)\n        || (nu[12] != 4u) || (nu[13] != 3u) || (nu[14] != 6u) || (nu[15] != 0u)\n        || (xs[0] != 9) || (xs[1] != 4) || (xs[2] != 10) || (xs[3] != 6)\n        || (xs[4] != 14) || (xs[5] != 11) || (xs[6] != 12) || (xs[7] != 15)\n        || (xs[8] != 7) || (xs[9] != 8) || (xs[10] != 12) || (xs[11] != 1)\n        || (xs[12] != 16) || (xs[13] != 10) || (xs[14] != 9) || (xs[15] != 0)\n        || (ns[0] != 1) || (ns[1] != -3) || (ns[2] != -1) || (ns[3] != -2)\n        || (ns[4] != -5) || (ns[5] != 7) || (ns[6] != -13) || (ns[7] != -8)\n        || (ns[8] != 2) || (ns[9] != -5) || (ns[10] != 11) || (ns[11] != -13)\n        || (ns[12] != 4) || (ns[13] != 3) || (ns[14] != 6) || (ns[15] != -17);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_reduce_nm",
        source: "vector_reduce_nm.c",
        driver: "#include <arm_neon.h>\nextern float maxnm4f_reduce(float32x4_t);\nextern float minnm4f_reduce(float32x4_t);\nint main(void) {\n    float32x4_t a = {1.0f, 9.0f, -3.0f, 4.0f};\n    return (maxnm4f_reduce(a) != 9.0f) || (minnm4f_reduce(a) != -3.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fmla",
        source: "vector_fmla.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t mla4f(float32x4_t, float32x4_t, float32x4_t);\nextern float32x4_t mls4f(float32x4_t, float32x4_t, float32x4_t);\nextern float32x4_t neg4f(float32x4_t);\nint main(void) {\n    float32x4_t a = {100.0f, 100.0f, 100.0f, 100.0f};\n    float32x4_t b = {1.0f, 2.0f, 3.0f, 4.0f};\n    float32x4_t c = {10.0f, 20.0f, 30.0f, 40.0f};\n    float32x4_t x = mla4f(a, b, c);\n    float32x4_t y = mls4f(a, b, c);\n    float32x4_t z = neg4f(c);\n    return (x[0] != 110.0f) || (x[1] != 140.0f) || (x[2] != 190.0f) || (x[3] != 260.0f)\n        || (y[0] != 90.0f) || (y[1] != 60.0f) || (y[2] != 10.0f) || (y[3] != -60.0f)\n        || (z[0] != -10.0f) || (z[1] != -20.0f) || (z[2] != -30.0f) || (z[3] != -40.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_unary",
        source: "vector_fp_unary.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t abs4f(float32x4_t);\nextern float32x4_t negabs4f(float32x4_t);\nextern float32x4_t sqrt4f(float32x4_t);\nint main(void) {\n    float32x4_t a = {-1.0f, 2.0f, -3.0f, 4.0f};\n    float32x4_t squares = {1.0f, 4.0f, 9.0f, 16.0f};\n    float32x4_t x = abs4f(a);\n    float32x4_t y = negabs4f(a);\n    float32x4_t z = sqrt4f(squares);\n    return (x[0] != 1.0f) || (x[1] != 2.0f) || (x[2] != 3.0f) || (x[3] != 4.0f)\n        || (y[0] != -1.0f) || (y[1] != -2.0f) || (y[2] != -3.0f) || (y[3] != -4.0f)\n        || (z[0] != 1.0f) || (z[1] != 2.0f) || (z[2] != 3.0f) || (z[3] != 4.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_unary_2d",
        source: "vector_fp_unary_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t abs2f(float64x2_t);\nextern float64x2_t neg2f(float64x2_t);\nextern float64x2_t sqrt2f(float64x2_t);\nint main(void) {\n    float64x2_t a = {-1.5, -4.0};\n    float64x2_t x = abs2f(a);\n    float64x2_t y = neg2f(a);\n    float64x2_t z = sqrt2f((float64x2_t){4.0, 9.0});\n    return (x[0] != 1.5) || (x[1] != 4.0)\n        || (y[0] != 1.5) || (y[1] != 4.0)\n        || (z[0] != 2.0) || (z[1] != 3.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_convert",
        source: "vector_fp_convert.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t to_float_s32(int32x4_t);\nextern float32x4_t to_float_u32(uint32x4_t);\nextern int32x4_t to_int_s32(float32x4_t);\nextern uint32x4_t to_uint_u32(float32x4_t);\nint main(void) {\n    int32x4_t si = {-1, 2, -3, 4};\n    uint32x4_t ui = {1u, 2u, 3u, 4u};\n    float32x4_t sf = {-1.0f, 2.0f, -3.0f, 4.0f};\n    float32x4_t uf = {1.0f, 2.0f, 3.0f, 4.0f};\n    float32x4_t xs = to_float_s32(si);\n    float32x4_t xu = to_float_u32(ui);\n    int32x4_t ys = to_int_s32(sf);\n    uint32x4_t yu = to_uint_u32(uf);\n    return (xs[0] != -1.0f) || (xs[1] != 2.0f) || (xs[2] != -3.0f) || (xs[3] != 4.0f)\n        || (xu[0] != 1.0f) || (xu[1] != 2.0f) || (xu[2] != 3.0f) || (xu[3] != 4.0f)\n        || (ys[0] != -1) || (ys[1] != 2) || (ys[2] != -3) || (ys[3] != 4)\n        || (yu[0] != 1u) || (yu[1] != 2u) || (yu[2] != 3u) || (yu[3] != 4u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_convert_2d",
        source: "vector_fp_convert_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t to_float_s64(int64x2_t);\nextern float64x2_t to_float_u64(uint64x2_t);\nextern int64x2_t to_int_s64(float64x2_t);\nextern uint64x2_t to_uint_u64(float64x2_t);\nint main(void) {\n    int64x2_t si = {-1, 2};\n    uint64x2_t ui = {1u, 2u};\n    float64x2_t sf = {-1.0, 2.0};\n    float64x2_t uf = {1.0, 2.0};\n    float64x2_t xs = to_float_s64(si);\n    float64x2_t xu = to_float_u64(ui);\n    int64x2_t ys = to_int_s64(sf);\n    uint64x2_t yu = to_uint_u64(uf);\n    return (xs[0] != -1.0) || (xs[1] != 2.0)\n        || (xu[0] != 1.0) || (xu[1] != 2.0)\n        || (ys[0] != -1) || (ys[1] != 2)\n        || (yu[0] != 1u) || (yu[1] != 2u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_recip",
        source: "vector_fp_recip.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t recip_est(float32x4_t);\nextern float32x4_t recip_step(float32x4_t, float32x4_t);\nextern float32x4_t rsqrt_est(float32x4_t);\nextern float32x4_t rsqrt_step(float32x4_t, float32x4_t);\nint main(void) {\n    float32x4_t ones = {1.0f, 1.0f, 1.0f, 1.0f};\n    float32x4_t re = recip_est(ones);\n    float32x4_t rs = recip_step(ones, ones);\n    float32x4_t se = rsqrt_est(ones);\n    float32x4_t ss = rsqrt_step(ones, ones);\n    return (re[0] < 0.99f) || (re[0] > 1.01f) || (re[1] < 0.99f) || (re[1] > 1.01f)\n        || (re[2] < 0.99f) || (re[2] > 1.01f) || (re[3] < 0.99f) || (re[3] > 1.01f)\n        || (rs[0] != 1.0f) || (rs[1] != 1.0f) || (rs[2] != 1.0f) || (rs[3] != 1.0f)\n        || (se[0] < 0.99f) || (se[0] > 1.01f) || (se[1] < 0.99f) || (se[1] > 1.01f)\n        || (se[2] < 0.99f) || (se[2] > 1.01f) || (se[3] < 0.99f) || (se[3] > 1.01f)\n        || (ss[0] != 1.0f) || (ss[1] != 1.0f) || (ss[2] != 1.0f) || (ss[3] != 1.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_round",
        source: "vector_fp_round.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t round_nearest(float32x4_t);\nextern float32x4_t round_down(float32x4_t);\nextern float32x4_t round_up(float32x4_t);\nextern float32x4_t round_zero(float32x4_t);\nint main(void) {\n    float32x4_t a = {1.2f, -1.7f, 2.25f, -3.8f};\n    float32x4_t rn = round_nearest(a);\n    float32x4_t rd = round_down(a);\n    float32x4_t ru = round_up(a);\n    float32x4_t rz = round_zero(a);\n    return (rn[0] != 1.0f) || (rn[1] != -2.0f) || (rn[2] != 2.0f) || (rn[3] != -4.0f)\n        || (rd[0] != 1.0f) || (rd[1] != -2.0f) || (rd[2] != 2.0f) || (rd[3] != -4.0f)\n        || (ru[0] != 2.0f) || (ru[1] != -1.0f) || (ru[2] != 3.0f) || (ru[3] != -3.0f)\n        || (rz[0] != 1.0f) || (rz[1] != -1.0f) || (rz[2] != 2.0f) || (rz[3] != -3.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_round_tail",
        source: "vector_fp_round_tail.c",
        driver: "#include <arm_neon.h>\nextern float32x4_t round_away(float32x4_t);\nextern float32x4_t round_current(float32x4_t);\nint main(void) {\n    float32x4_t a = {1.2f, -1.7f, 2.5f, -3.5f};\n    float32x4_t ra = round_away(a);\n    float32x4_t rc = round_current(a);\n    return (ra[0] != 1.0f) || (ra[1] != -2.0f) || (ra[2] != 3.0f) || (ra[3] != -4.0f)\n        || (rc[0] != 1.0f) || (rc[1] != -2.0f) || (rc[2] != 2.0f) || (rc[3] != -4.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_round_2d",
        source: "vector_fp_round_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t round_nearest2(float64x2_t);\nextern float64x2_t round_down2(float64x2_t);\nextern float64x2_t round_up2(float64x2_t);\nextern float64x2_t round_zero2(float64x2_t);\nextern float64x2_t round_away2(float64x2_t);\nextern float64x2_t round_current2(float64x2_t);\nint main(void) {\n    float64x2_t a = {1.2, -1.7};\n    float64x2_t rn = round_nearest2(a);\n    float64x2_t rd = round_down2(a);\n    float64x2_t ru = round_up2(a);\n    float64x2_t rz = round_zero2(a);\n    float64x2_t ra = round_away2(a);\n    float64x2_t rc = round_current2((float64x2_t){2.5, -3.5});\n    return (rn[0] != 1.0) || (rn[1] != -2.0)\n        || (rd[0] != 1.0) || (rd[1] != -2.0)\n        || (ru[0] != 2.0) || (ru[1] != -1.0)\n        || (rz[0] != 1.0) || (rz[1] != -1.0)\n        || (ra[0] != 1.0) || (ra[1] != -2.0)\n        || (rc[0] != 2.0) || (rc[1] != -4.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fp_recip_2d",
        source: "vector_fp_recip_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t recip_est2(float64x2_t);\nextern float64x2_t recip_step2(float64x2_t, float64x2_t);\nextern float64x2_t rsqrt_est2(float64x2_t);\nextern float64x2_t rsqrt_step2(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t ones = {1.0, 1.0};\n    float64x2_t re = recip_est2(ones);\n    float64x2_t rs = recip_step2(ones, ones);\n    float64x2_t se = rsqrt_est2(ones);\n    float64x2_t ss = rsqrt_step2(ones, ones);\n    return (re[0] < 0.99) || (re[0] > 1.01) || (re[1] < 0.99) || (re[1] > 1.01)\n        || (rs[0] != 1.0) || (rs[1] != 1.0)\n        || (se[0] < 0.99) || (se[0] > 1.01) || (se[1] < 0.99) || (se[1] > 1.01)\n        || (ss[0] != 1.0) || (ss[1] != 1.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fcmp_2d",
        source: "vector_fcmp_2d.c",
        driver: "#include <arm_neon.h>\nextern uint64x2_t eq_mask_f64(float64x2_t, float64x2_t);\nextern uint64x2_t ge_mask_f64(float64x2_t, float64x2_t);\nextern uint64x2_t gt_mask_f64(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t a = {1.0, 9.0};\n    float64x2_t b = {1.0, 7.0};\n    uint64x2_t eq = eq_mask_f64(a, b);\n    uint64x2_t ge = ge_mask_f64(a, b);\n    uint64x2_t gt = gt_mask_f64(a, b);\n    return (eq[0] != 0xFFFFFFFFFFFFFFFFull) || (eq[1] != 0ull)\n        || (ge[0] != 0xFFFFFFFFFFFFFFFFull) || (ge[1] != 0xFFFFFFFFFFFFFFFFull)\n        || (gt[0] != 0ull) || (gt[1] != 0xFFFFFFFFFFFFFFFFull);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_minmax_2d",
        source: "vector_minmax_2d.c",
        driver: "#include <arm_neon.h>\nextern float64x2_t max2d(float64x2_t, float64x2_t);\nextern float64x2_t min2d(float64x2_t, float64x2_t);\nint main(void) {\n    float64x2_t a = {1.0, 9.0};\n    float64x2_t b = {2.0, 7.0};\n    float64x2_t x = max2d(a, b);\n    float64x2_t n = min2d(a, b);\n    return (x[0] != 2.0) || (x[1] != 9.0)\n        || (n[0] != 1.0) || (n[1] != 7.0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_fcmp",
        source: "vector_fcmp.c",
        driver: "#include <arm_neon.h>\nextern uint32x4_t eq_mask_f32(float32x4_t, float32x4_t);\nextern uint32x4_t ge_mask_f32(float32x4_t, float32x4_t);\nextern uint32x4_t gt_mask_f32(float32x4_t, float32x4_t);\nint main(void) {\n    float32x4_t a = {1.0f, 9.0f, -3.0f, 4.0f};\n    float32x4_t b = {1.0f, 7.0f, -5.0f, 8.0f};\n    uint32x4_t eq = eq_mask_f32(a, b);\n    uint32x4_t ge = ge_mask_f32(a, b);\n    uint32x4_t gt = gt_mask_f32(a, b);\n    return (eq[0] != 0xFFFFFFFFu) || (eq[1] != 0u) || (eq[2] != 0u) || (eq[3] != 0u)\n        || (ge[0] != 0xFFFFFFFFu) || (ge[1] != 0xFFFFFFFFu) || (ge[2] != 0xFFFFFFFFu) || (ge[3] != 0u)\n        || (gt[0] != 0u) || (gt[1] != 0xFFFFFFFFu) || (gt[2] != 0xFFFFFFFFu) || (gt[3] != 0u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_shuffle",
        source: "vector_shuffle.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f swap_halves(v4f);\nextern v4f blend_even(v4f, v4f);\nint main(void) {\n    v4f a = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f b = {10.0f, 20.0f, 30.0f, 40.0f};\n    v4f s = swap_halves(a);\n    v4f t = blend_even(a, b);\n    return (s[0] != 3.0f) || (s[1] != 4.0f) || (s[2] != 1.0f) || (s[3] != 2.0f)\n        || (t[0] != 1.0f) || (t[1] != 20.0f) || (t[2] != 3.0f) || (t[3] != 40.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_tbl",
        source: "vector_tbl.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f swap_halves_tbl(v4f);\nextern v4f blend_even_tbl(v4f, v4f);\nint main(void) {\n    v4f a = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f b = {10.0f, 20.0f, 30.0f, 40.0f};\n    v4f s = swap_halves_tbl(a);\n    v4f t = blend_even_tbl(a, b);\n    return (s[0] != 3.0f) || (s[1] != 4.0f) || (s[2] != 1.0f) || (s[3] != 2.0f)\n        || (t[0] != 1.0f) || (t[1] != 20.0f) || (t[2] != 3.0f) || (t[3] != 40.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_zip_uzp",
        source: "vector_zip_uzp.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f zip_lo(v4f, v4f);\nextern v4f zip_hi(v4f, v4f);\nextern v4f uzp_lo(v4f, v4f);\nextern v4f uzp_hi(v4f, v4f);\nextern v4f trn_lo(v4f, v4f);\nextern v4f trn_hi(v4f, v4f);\nint main(void) {\n    v4f a = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f b = {10.0f, 20.0f, 30.0f, 40.0f};\n    v4f zl = zip_lo(a, b);\n    v4f zh = zip_hi(a, b);\n    v4f ul = uzp_lo(a, b);\n    v4f uh = uzp_hi(a, b);\n    v4f tl = trn_lo(a, b);\n    v4f th = trn_hi(a, b);\n    return (zl[0] != 1.0f) || (zl[1] != 10.0f) || (zl[2] != 2.0f) || (zl[3] != 20.0f)\n        || (zh[0] != 3.0f) || (zh[1] != 30.0f) || (zh[2] != 4.0f) || (zh[3] != 40.0f)\n        || (ul[0] != 1.0f) || (ul[1] != 3.0f) || (ul[2] != 10.0f) || (ul[3] != 30.0f)\n        || (uh[0] != 2.0f) || (uh[1] != 4.0f) || (uh[2] != 20.0f) || (uh[3] != 40.0f)\n        || (tl[0] != 1.0f) || (tl[1] != 10.0f) || (tl[2] != 3.0f) || (tl[3] != 30.0f)\n        || (th[0] != 2.0f) || (th[1] != 20.0f) || (th[2] != 4.0f) || (th[3] != 40.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_dup",
        source: "vector_dup.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\ntypedef double v2d __attribute__((vector_size(16)));\ntypedef short v8h __attribute__((vector_size(16)));\ntypedef signed char v16b __attribute__((vector_size(16)));\nextern v4f dup_f32(v4f);\nextern v2d dup_f64(v2d);\nextern v8h dup_s16(v8h);\nextern v16b dup_s8(v16b);\nint main(void) {\n    v4f f = {1.0f, 2.0f, 3.0f, 4.0f};\n    v2d d = {1.5, 2.5};\n    v8h h = {1, 2, 3, 4, 5, 6, 7, 8};\n    v16b b = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16};\n    v4f rf = dup_f32(f);\n    v2d rd = dup_f64(d);\n    v8h rh = dup_s16(h);\n    v16b rb = dup_s8(b);\n    return (rf[0] != 4.0f) || (rf[1] != 4.0f) || (rf[2] != 4.0f) || (rf[3] != 4.0f)\n        || (rd[0] != 2.5) || (rd[1] != 2.5)\n        || (rh[0] != 8) || (rh[1] != 8) || (rh[2] != 8) || (rh[3] != 8)\n        || (rh[4] != 8) || (rh[5] != 8) || (rh[6] != 8) || (rh[7] != 8)\n        || (rb[0] != 16) || (rb[1] != 16) || (rb[2] != 16) || (rb[3] != 16)\n        || (rb[4] != 16) || (rb[5] != 16) || (rb[6] != 16) || (rb[7] != 16)\n        || (rb[8] != 16) || (rb[9] != 16) || (rb[10] != 16) || (rb[11] != 16)\n        || (rb[12] != 16) || (rb[13] != 16) || (rb[14] != 16) || (rb[15] != 16);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "vector_blend",
        source: "vector_blend.c",
        driver: "typedef unsigned char v16u8 __attribute__((vector_size(16)));\ntypedef unsigned v4u __attribute__((vector_size(16)));\nextern v16u8 blend_bytes(v16u8, v16u8, v16u8);\nextern v4u blend_u32(v4u, v4u, v4u);\nint main(void) {\n    v16u8 a = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16};\n    v16u8 b = {101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116};\n    v16u8 m = {0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00};\n    v4u au = {10u, 20u, 30u, 40u};\n    v4u bu = {110u, 120u, 130u, 140u};\n    v4u mu = {0xFFFFFFFFu, 0u, 0xFFFFFFFFu, 0u};\n    v16u8 rb = blend_bytes(a, b, m);\n    v4u ru = blend_u32(au, bu, mu);\n    return (rb[0] != 1) || (rb[1] != 102) || (rb[2] != 3) || (rb[3] != 104)\n        || (rb[4] != 5) || (rb[5] != 106) || (rb[6] != 7) || (rb[7] != 108)\n        || (rb[8] != 9) || (rb[9] != 110) || (rb[10] != 11) || (rb[11] != 112)\n        || (rb[12] != 13) || (rb[13] != 114) || (rb[14] != 15) || (rb[15] != 116)\n        || (ru[0] != 10u) || (ru[1] != 120u) || (ru[2] != 30u) || (ru[3] != 140u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "bitops",
        source: "bitops.c",
        driver: "extern unsigned bit_mix(unsigned);\nint main(void) { return bit_mix(0xABu) != 117u; }\n",
        support: None,
    },
    ProbeCase {
        name: "extern_puts",
        source: "extern_puts.c",
        driver: "extern int call_puts(void);\nint main(void) { return call_puts() < 0; }\n",
        support: None,
    },
    ProbeCase {
        name: "tls_global",
        source: "tls_global.c",
        driver: "extern int read_tls_plus_one(void);\nint main(void) { return read_tls_plus_one() != 6; }\n",
        support: None,
    },
    ProbeCase {
        name: "tls_addr",
        source: "tls_addr.c",
        driver: "extern int *tls_value_addr(void);\nint main(void) {\n    int *ptr = tls_value_addr();\n    *ptr = 7;\n    return (*ptr != 7) || (*tls_value_addr() != 7);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "tls_bss_global",
        source: "tls_bss_global.c",
        driver: "extern int bump_tls_counter(int);\nint main(void) {\n    return (bump_tls_counter(4) != 4) || (bump_tls_counter(3) != 7);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "tls_ptr_pass",
        source: "tls_ptr_pass.c",
        driver: "extern int bump_tls_via_ptr(void);\nint main(void) {\n    return (bump_tls_via_ptr() != 2) || (bump_tls_via_ptr() != 4);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics",
        source: "atomics.c",
        driver: "extern int add_and_fetch(int);\nextern int load_then_store(int);\nint main(void) {\n    return (add_and_fetch(4) != 4) || (load_then_store(7) != 4) || (add_and_fetch(1) != 12);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_fences",
        source: "atomic_fences.c",
        driver: "extern int fence_acquire(int *);\nextern void fence_release(int *, int);\nextern int fence_acq_rel(int *);\nextern int fence_seq_cst(int *);\nint main(void) {\n    int x = 5;\n    if (fence_acquire(&x) != 5) return 1;\n    fence_release(&x, 9);\n    if (x != 9) return 1;\n    if (fence_acq_rel(&x) != 9) return 1;\n    if (fence_seq_cst(&x) != 9) return 1;\n    return 0;\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics8",
        source: "atomics8.c",
        driver: "extern unsigned char load_then_store8(unsigned char);\nextern unsigned char swap8(unsigned char);\nint main(void) {\n    return (load_then_store8(4) != 0u)\n        || (load_then_store8(3) != 4u)\n        || (swap8(9) != 7u)\n        || (swap8(2) != 9u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics16",
        source: "atomics16.c",
        driver: "extern unsigned short load_then_store16(unsigned short);\nextern unsigned short swap16(unsigned short);\nint main(void) {\n    return (load_then_store16(400u) != 0u)\n        || (load_then_store16(300u) != 400u)\n        || (swap16(900u) != 700u)\n        || (swap16(200u) != 900u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_fetchadd_narrow",
        source: "atomic_fetchadd_narrow.c",
        driver: "extern unsigned char add8(unsigned char);\nextern unsigned short add16(unsigned short);\nint main(void) {\n    return (add8(4u) != 0u)\n        || (add8(3u) != 4u)\n        || (add16(400u) != 0u)\n        || (add16(300u) != 400u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_cas_narrow",
        source: "atomic_cas_narrow.c",
        driver: "extern unsigned char cas8(unsigned char, unsigned char);\nextern unsigned short cas16(unsigned short, unsigned short);\nint main(void) {\n    return (cas8(0u, 7u) != 0u)\n        || (cas8(0u, 11u) != 7u)\n        || (cas16(0u, 700u) != 0u)\n        || (cas16(0u, 300u) != 700u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_exchange_cas",
        source: "atomic_exchange_cas.c",
        driver: "extern int swap_acqrel(int);\nextern long swap64_acqrel(long);\nextern int cas_acqrel(int, int);\nextern long cas64_acqrel(long, long);\nint main(void) {\n    return (swap_acqrel(4) != 0)\n        || (swap_acqrel(7) != 4)\n        || (cas_acqrel(7, 9) != 7)\n        || (cas_acqrel(0, 11) != 9)\n        || (swap64_acqrel(10) != 0)\n        || (swap64_acqrel(15) != 10)\n        || (cas64_acqrel(15, 21) != 15)\n        || (cas64_acqrel(0, 31) != 21);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_bitops",
        source: "atomic_bitops.c",
        driver: "extern unsigned fetch_or_mask(unsigned);\nextern unsigned fetch_xor_mask(unsigned);\nextern unsigned fetch_and_mask(unsigned);\nint main(void) {\n    return (fetch_or_mask(3u) != 0u)\n        || (fetch_or_mask(4u) != 3u)\n        || (fetch_xor_mask(1u) != 7u)\n        || (fetch_and_mask(5u) != 6u)\n        || (fetch_and_mask(1u) != 4u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_bitops_narrow",
        source: "atomic_bitops_narrow.c",
        driver: "extern unsigned char or8(unsigned char);\nextern unsigned char xor8(unsigned char);\nextern unsigned char and8(unsigned char);\nextern unsigned short or16(unsigned short);\nextern unsigned short xor16(unsigned short);\nextern unsigned short and16(unsigned short);\nint main(void) {\n    return (or8(3u) != 0u)\n        || (xor8(1u) != 3u)\n        || (and8(1u) != 2u)\n        || (or16(0x30u) != 0u)\n        || (xor16(0x10u) != 0x30u)\n        || (and16(0x10u) != 0x20u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_maxmin",
        source: "atomic_maxmin.c",
        driver: "extern int fetch_max_builtin(int);\nextern int fetch_min_builtin(int);\nint main(void) {\n    return (fetch_max_builtin(4) != 0)\n        || (fetch_max_builtin(2) != 4)\n        || (fetch_min_builtin(3) != 4)\n        || (fetch_min_builtin(5) != 3);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_maxmin_narrow",
        source: "atomic_maxmin_narrow.c",
        driver: "extern unsigned char max8(unsigned char);\nextern unsigned char min8(unsigned char);\nextern unsigned short max16(unsigned short);\nextern unsigned short min16(unsigned short);\nint main(void) {\n    return (max8(7u) != 0u)\n        || (min8(3u) != 7u)\n        || (max16(700u) != 0u)\n        || (min16(300u) != 700u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "compare_chain",
        source: "compare_chain.c",
        driver: "extern int both_small(int, int);\nint main(void) {\n    return (both_small(1, 2) != 2) || (both_small(3, 9) != 1) || (both_small(30, 40) != 0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "copy_until_zero",
        source: "copy_until_zero.c",
        driver: "extern int copy_until_zero(char *, const char *);\nint main(void) {\n    char buf[8] = {0};\n    int n = copy_until_zero(buf, \"cat\");\n    return (n != 3) || (buf[0] != 'c') || (buf[1] != 'a') || (buf[2] != 't') || (buf[3] != 0);\n}\n",
        support: None,
    },
];

fn probe_source_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("clang_probe")
        .join(name)
}

fn format_command_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("\nstdout:\n{}", stdout),
        (true, false) => format!("\nstderr:\n{}", stderr),
        (false, false) => format!("\nstdout:\n{}\nstderr:\n{}", stdout, stderr),
    }
}

fn run_command_output(
    command: &mut Command,
    context: &str,
    timeout: Duration,
) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("spawn {}: {}", context, err))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|err| format!("wait for {}: {}", context, err));
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|err| format!("wait for timed out {}: {}", context, err))?;
                return Err(format!(
                    "{} timed out after {:.1?}{}",
                    context,
                    timeout,
                    format_command_output(&output)
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(err) => return Err(format!("poll {}: {}", context, err)),
        }
    }
}

fn clang_generate_asm(source: &Path, opt: &str, output: &Path) -> Result<(), String> {
    let opt_flag = format!("-{}", opt);
    let mut cmd = Command::new("clang");
    cmd.arg("-target")
        .arg("arm64-apple-macos11")
        .arg("-S")
        .arg(&opt_flag)
        .arg("-o")
        .arg(output)
        .arg(source);
    let result = run_command_output(
        &mut cmd,
        &format!("clang -S for {} {}", source.display(), opt),
        COMMAND_TIMEOUT,
    )?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang -S failed for {} {}:{}",
            source.display(),
            opt,
            format_command_output(&result)
        ))
    }
}

fn clang_compile_object(source: &Path, output: &Path) -> Result<(), String> {
    let mut cmd = Command::new("clang");
    cmd.arg("-target")
        .arg("arm64-apple-macos11")
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(output);
    let result = run_command_output(
        &mut cmd,
        &format!("clang -c for {}", source.display()),
        COMMAND_TIMEOUT,
    )?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang -c failed for {}:{}",
            source.display(),
            format_command_output(&result)
        ))
    }
}

fn assemble_with_ours(src: &str, output: &Path) -> Result<(), String> {
    let obj = afs_as::assemble::assemble_source(src)
        .map_err(|err| format!("afs-as assemble failed:\n{}", err))?;
    let mut file =
        fs::File::create(output).map_err(|err| format!("create {}: {}", output.display(), err))?;
    afs_as::macho::write_macho(&obj, &mut file)
        .map_err(|err| format!("write {}: {}", output.display(), err))
}

fn assemble_with_system(src_path: &Path, output: &Path) -> Result<(), String> {
    let mut cmd = Command::new("as");
    cmd.arg("-o").arg(output).arg(src_path);
    let result = run_command_output(
        &mut cmd,
        &format!("system as for {}", src_path.display()),
        COMMAND_TIMEOUT,
    )?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "system as failed for {}:{}",
            src_path.display(),
            format_command_output(&result)
        ))
    }
}

fn clang_link_binary(objects: &[&Path], output: &Path) -> Result<(), String> {
    let mut cmd = Command::new("clang");
    cmd.arg("-target").arg("arm64-apple-macos11");
    for object in objects {
        cmd.arg(object);
    }
    cmd.arg("-o").arg(output);
    let result = run_command_output(
        &mut cmd,
        &format!("clang link for {}", output.display()),
        COMMAND_TIMEOUT,
    )?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang link failed for {}:{}",
            output.display(),
            format_command_output(&result)
        ))
    }
}

fn run_binary_with_timeout(path: &Path) -> Result<(i32, String, String), String> {
    let mut cmd = Command::new(path);
    let output = run_command_output(
        &mut cmd,
        &format!("run binary {}", path.display()),
        COMMAND_TIMEOUT,
    )?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn compare_object_semantics(ours: &Path, reference: &Path) -> Result<(), String> {
    let ours_text = common::object_text_bytes(ours);
    let ref_text = common::object_text_bytes(reference);
    if ours_text != ref_text {
        return Err(format!(
            "text bytes differ\nours: {:02X?}\nref:  {:02X?}",
            ours_text, ref_text
        ));
    }

    let ours_load = normalize_tool_output(&common::object_load_commands(ours));
    let ref_load = normalize_tool_output(&common::object_load_commands(reference));
    if ours_load != ref_load {
        return Err(format!(
            "load commands differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_load, ref_load
        ));
    }

    let ours_relocs = normalize_tool_output(&common::object_relocations(ours));
    let ref_relocs = normalize_tool_output(&common::object_relocations(reference));
    if ours_relocs != ref_relocs {
        return Err(format!(
            "relocations differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_relocs, ref_relocs
        ));
    }

    let ours_symbols = normalize_tool_output(&common::object_symbols(ours));
    let ref_symbols = normalize_tool_output(&common::object_symbols(reference));
    if ours_symbols != ref_symbols {
        return Err(format!(
            "symbols differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_symbols, ref_symbols
        ));
    }

    let ours_symbols_verbose = normalize_tool_output(&common::object_symbols_verbose(ours));
    let ref_symbols_verbose = normalize_tool_output(&common::object_symbols_verbose(reference));
    if ours_symbols_verbose != ref_symbols_verbose {
        return Err(format!(
            "verbose symbols differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_symbols_verbose, ref_symbols_verbose
        ));
    }

    Ok(())
}

fn run_probe(case: &ProbeCase, opt: &'static str) -> ProbeResult {
    let mut result = ProbeResult {
        case: case.name,
        opt,
        status: StageStatus::default(),
        detail: None,
    };

    let paths = common::TempPaths::new("afs_clang_probe");
    let root = paths.asm.parent().expect("temp root").to_path_buf();
    let source = probe_source_path(case.source);
    if let Err(err) = clang_generate_asm(&source, opt, &paths.asm) {
        result.detail = Some(err);
        return result;
    }

    let asm = match fs::read_to_string(&paths.asm) {
        Ok(asm) => asm,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.asm.display(), err));
            return result;
        }
    };

    match afs_as::parse::parse(&asm) {
        Ok(_) => result.status.parse = true,
        Err(err) => {
            result.detail = Some(format!("parse failed:\n{}", err));
            return result;
        }
    }

    if let Err(err) = assemble_with_ours(&asm, &paths.obj) {
        result.detail = Some(err);
        return result;
    }
    if let Err(err) = assemble_with_system(&paths.asm, &paths.ref_obj) {
        result.detail = Some(err);
        return result;
    }
    result.status.assemble = true;

    if let Err(err) = compare_object_semantics(&paths.obj, &paths.ref_obj) {
        result.detail = Some(err);
        return result;
    }
    result.status.semantics = true;

    let ours_obj = match fs::read(&paths.obj) {
        Ok(data) => data,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.obj.display(), err));
            return result;
        }
    };
    let ref_obj = match fs::read(&paths.ref_obj) {
        Ok(data) => data,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.ref_obj.display(), err));
            return result;
        }
    };
    if ours_obj != ref_obj {
        result.detail = Some("raw object bytes differ".into());
        return result;
    }
    result.status.raw = true;

    let driver_c = root.join("driver.c");
    let driver_o = root.join("driver.o");
    if let Err(err) = fs::write(&driver_c, case.driver) {
        result.detail = Some(format!("write {}: {}", driver_c.display(), err));
        return result;
    }
    if let Err(err) = clang_compile_object(&driver_c, &driver_o) {
        result.detail = Some(err);
        return result;
    }

    let mut link_inputs = vec![paths.obj.as_path(), driver_o.as_path()];
    let mut ref_link_inputs = vec![paths.ref_obj.as_path(), driver_o.as_path()];
    let support_o = root.join("support.o");
    if let Some(support_src) = case.support {
        let support_c = root.join("support.c");
        if let Err(err) = fs::write(&support_c, support_src) {
            result.detail = Some(format!("write {}: {}", support_c.display(), err));
            return result;
        }
        if let Err(err) = clang_compile_object(&support_c, &support_o) {
            result.detail = Some(err);
            return result;
        }
        link_inputs.push(support_o.as_path());
        ref_link_inputs.push(support_o.as_path());
    }

    let ref_bin = root.join("ref-out");
    if let Err(err) = clang_link_binary(&link_inputs, &paths.bin) {
        result.detail = Some(err);
        return result;
    }
    if let Err(err) = clang_link_binary(&ref_link_inputs, &ref_bin) {
        result.detail = Some(err);
        return result;
    }
    result.status.link = true;

    let (our_code, our_stdout, our_stderr) = match run_binary_with_timeout(&paths.bin) {
        Ok(output) => output,
        Err(err) => {
            result.detail = Some(err);
            return result;
        }
    };
    let (ref_code, ref_stdout, ref_stderr) = match run_binary_with_timeout(&ref_bin) {
        Ok(output) => output,
        Err(err) => {
            result.detail = Some(err);
            return result;
        }
    };
    if our_code != 0 || ref_code != 0 {
        result.detail = Some(format!(
            "unexpected exit codes ours={} ref={}\nours stderr:\n{}\nref stderr:\n{}",
            our_code, ref_code, our_stderr, ref_stderr
        ));
        return result;
    }
    if our_stdout != ref_stdout || our_stderr != ref_stderr {
        result.detail = Some(format!(
            "runtime output differs\n--- ours stdout ---\n{}\n--- ref stdout ---\n{}\n--- ours stderr ---\n{}\n--- ref stderr ---\n{}",
            our_stdout, ref_stdout, our_stderr, ref_stderr
        ));
        return result;
    }
    result.status.run = true;

    result
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:") && !line.trim_end().ends_with(":"))
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mark(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "--"
    }
}

fn render_dashboard(results: &[ProbeResult]) -> String {
    let mut out = String::from("case           opt parse asm sem raw link run\n");
    for result in results {
        out.push_str(&format!(
            "{:<14} {:<2} {:<5} {:<3} {:<3} {:<3} {:<4} {:<3}\n",
            result.case,
            result.opt,
            mark(result.status.parse),
            mark(result.status.assemble),
            mark(result.status.semantics),
            mark(result.status.raw),
            mark(result.status.link),
            mark(result.status.run),
        ));
    }

    let failures: Vec<_> = results.iter().filter(|result| !result.status.run).collect();
    if !failures.is_empty() {
        out.push_str("\nFailures:\n");
        for failure in failures {
            out.push_str(&format!(
                "\n[{} {}]\n{}\n",
                failure.case,
                failure.opt,
                failure.detail.as_deref().unwrap_or("missing detail")
            ));
        }
    }

    out
}

#[test]
fn clang_probe_dashboard() {
    let case_filter = std::env::var("AFS_CLANG_PROBE_CASE").ok();
    let opt_filter = std::env::var("AFS_CLANG_PROBE_OPT").ok();
    let fail_fast = std::env::var("AFS_CLANG_PROBE_FAIL_FAST")
        .ok()
        .is_some_and(|value| value != "0");
    let mut results = Vec::new();
    let mut matched_any = false;
    for case in CASES.iter().filter(|case| {
        case_filter
            .as_deref()
            .is_none_or(|filter| case.name.contains(filter))
    }) {
        for opt in ["O0", "O2"]
            .into_iter()
            .filter(|opt| opt_filter.as_deref().is_none_or(|filter| *opt == filter))
        {
            matched_any = true;
            eprintln!("probe {} {}: start", case.name, opt);
            let start = Instant::now();
            let probe = run_probe(case, opt);
            let status = if probe.status.run { "ok" } else { "fail" };
            eprintln!(
                "probe {} {}: {} in {:.2?}",
                case.name,
                opt,
                status,
                start.elapsed()
            );
            let failed = !probe.status.run;
            results.push(probe);
            if failed && fail_fast {
                panic!(
                    "clang probe dashboard failed:\n{}",
                    render_dashboard(&results)
                );
            }
        }
    }
    assert!(
        matched_any,
        "no clang probe cases matched filters case={:?} opt={:?}",
        case_filter, opt_filter
    );

    let failures: Vec<_> = results.iter().filter(|result| !result.status.run).collect();
    assert!(
        failures.is_empty(),
        "clang probe dashboard failed:\n{}",
        render_dashboard(&results)
    );
}
