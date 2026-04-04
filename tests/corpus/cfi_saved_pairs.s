.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _keep
.p2align 2
_keep:
.cfi_startproc
stp x22, x21, [sp, #-48]!
stp x20, x19, [sp, #16]
stp x29, x30, [sp, #32]
add x29, sp, #32
.cfi_def_cfa w29, 16
.cfi_offset w30, -8
.cfi_offset w29, -16
.cfi_offset w19, -24
.cfi_offset w20, -32
.cfi_offset w21, -40
.cfi_offset w22, -48
mov x19, x0
mov x20, x1
add x0, x20, x19
ldp x29, x30, [sp, #32]
ldp x20, x19, [sp, #16]
ldp x22, x21, [sp], #48
ret
.cfi_endproc
.subsections_via_symbols
