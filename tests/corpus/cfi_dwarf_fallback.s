.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _h
.p2align 2
_h:
.cfi_startproc
sub sp, sp, #32
stp x29, x30, [sp, #16]
add x29, sp, #16
.cfi_def_cfa w29, 16
.cfi_offset w30, -8
.cfi_offset w29, -16
str x19, [sp, #8]
.cfi_offset w19, -24
ldr x19, [sp, #8]
ldp x29, x30, [sp, #16]
add sp, sp, #32
ret
.cfi_endproc
.subsections_via_symbols
