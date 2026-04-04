.text
.globl _fp_load_store_surface
_fp_load_store_surface:
    ldr d0, [x1]
    str d2, [x3, #16]
    ldr s4, [x5, x6]
    str s7, [x8, w9, uxtw #2]
    ldr d10, 1f
    ldr s11, 2f
    ldr d12, [sp], #8
    str s13, [sp, #-8]!
    ret
    .p2align 3
1:
    .quad 0
2:
    .word 0
