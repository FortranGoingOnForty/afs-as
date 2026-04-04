.text
.globl _extended_addsub
_extended_addsub:
    add x0, x0, w1, sxtw
    add x2, x2, w3, sxtw #3
    sub x4, x5, w6, uxtw #2
    cmp x7, w8, sxtw
    cmn x9, w10, sxtw #3
    add x11, sp, w12, sxtw #2
    ret
