.build_version macos, 15, 0
.text
.globl _addressing_surface
_addressing_surface:
    neg x0, x1
    mvn x2, x3
    cset x4, eq
    cinc w5, w6, ne
    ldr x7, [x8, x9]
    ldr x10, [x11, w12, uxtw #3]
    str x13, [x14, x15]
    ldrb w16, [x17, x18]
    ldrh w19, [x20, w21, uxtw #1]
    ldrsw x22, [x23, w24, sxtw #2]
    ldrsw x25, .Llit32
    ldr x26, .Llit64
    ret
    .p2align 3
.Llit32:
    .word -1
    .word 0
.Llit64:
    .quad 42
