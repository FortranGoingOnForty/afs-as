; Workbench note: Apple `as` encoding probe for the ZIP/UZP/TRN interleave and
; deinterleave family that led to the corresponding Sprint 13 SIMD slice.
.text
zip1.4s v0, v0, v1
zip2.4s v2, v3, v4
uzp1.4s v5, v6, v7
uzp2.4s v8, v9, v10
trn1.4s v11, v12, v13
trn2.4s v14, v15, v16
