// Workbench note: this family was promoted after the SIMD conversion probe
// showed that clang was emitting the compact .4s conversion neighborhood.
// Reference encodings pinned during promotion:
//   scvtf.4s v0, v1   -> 0x4E21D820
//   ucvtf.4s v2, v3   -> 0x6E21D862
//   fcvtzs.4s v4, v5  -> 0x4EA1B8A4
//   fcvtzu.4s v6, v7  -> 0x6EA1B8E6

.text
scvtf.4s v0, v1
ucvtf.4s v2, v3
fcvtzs.4s v4, v5
fcvtzu.4s v6, v7
