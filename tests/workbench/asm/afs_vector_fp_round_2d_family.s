// Workbench note: Apple `as` encoding pin for the float64 SIMD rounding family
// emitted by clang from Neon intrinsics.
.text
frintn.2d v0, v0
frintm.2d v1, v2
frintp.2d v3, v4
frintz.2d v5, v6
frinta.2d v7, v8
frinti.2d v9, v10
