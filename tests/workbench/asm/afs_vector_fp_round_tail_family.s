// Workbench note: this tail family was promoted after the Neon rounding probe
// showed that clang was still emitting two SIMD FP rounding siblings beyond
// the core `frintn/m/p/z` surface.
// Reference encodings pinned during promotion:
//   frinta.4s v0, v1 -> 0x6E218820
//   frinti.4s v2, v3 -> 0x6EA19862

.text
frinta.4s v0, v1
frinti.4s v2, v3
