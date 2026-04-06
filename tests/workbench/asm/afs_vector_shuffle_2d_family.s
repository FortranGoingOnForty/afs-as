// Workbench note: this probe pins the float64 `.2d` shuffle/interleave
// encodings Apple `as` accepts after clang exposed `zip1.2d` / `zip2.2d`
// directly from float64 Neon intrinsics. It led to adding the `.2d` shuffle
// sibling family and promoting `tests/clang_probe/vector_shuffle_2d.c`.
//   zip1.2d v0, v1, v2   -> 0x4EC23820
//   zip2.2d v3, v4, v5   -> 0x4EC57883
//   uzp1.2d v6, v7, v8   -> 0x4EC818E6
//   uzp2.2d v9, v10, v11 -> 0x4ECB5949
//   trn1.2d v12, v13, v14 -> 0x4ECE29AC
//   trn2.2d v15, v16, v17 -> 0x4ED16A0F

.text
zip1.2d v0, v1, v2
zip2.2d v3, v4, v5
uzp1.2d v6, v7, v8
uzp2.2d v9, v10, v11
trn1.2d v12, v13, v14
trn2.2d v15, v16, v17
