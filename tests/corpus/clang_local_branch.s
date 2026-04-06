.section __TEXT,__text,regular,pure_instructions
.build_version macos, 15, 0 sdk_version 15, 5
.globl _pick_positive_max
.p2align 2
_pick_positive_max:
.cfi_startproc
    cmp w0, #0
    b.le LBB0_2
    cmp w0, w1
    csel w0, w0, w1, gt
    ret
LBB0_2:
    mov w0, #0
    ret
.cfi_endproc

.subsections_via_symbols
