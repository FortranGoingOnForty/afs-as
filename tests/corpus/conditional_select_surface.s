.build_version macos, 15, 0
.text
.globl _conditional_select_surface
_conditional_select_surface:
    csel w0, w0, w1, gt
    csinc x2, x3, x4, ne
    csinv x5, x6, x7, mi
    csneg x8, x9, x10, lt
    cset x11, eq
    csetm w12, pl
    cinc w13, w14, hi
    cinv x15, x16, ls
    cneg x17, x18, ge
    ret
