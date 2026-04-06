// Workbench note: this family was promoted after the Neon rounding probe
// showed that clang was emitting a compact SIMD FP rounding neighborhood.
// Reference encodings pinned during promotion:
//   frintn.4s v0, v1 -> 0x4E218820
//   frintm.4s v2, v3 -> 0x4E219862
//   frintp.4s v4, v5 -> 0x4EA188A4
//   frintz.4s v6, v7 -> 0x4EA198E6

.text
frintn.4s v0, v1
frintm.4s v2, v3
frintp.4s v4, v5
frintz.4s v6, v7
