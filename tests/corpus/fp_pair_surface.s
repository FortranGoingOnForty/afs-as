.text
.globl _fp_pair_surface
_fp_pair_surface:
    ldp d8, d9, [sp, #-16]!
    stp d10, d11, [sp], #16
    ldp d12, d13, [sp, #32]
    stp s0, s1, [sp], #8
    ldp s2, s3, [sp, #-8]!
    ldp s4, s5, [sp, #16]
    ret
