.text
.globl _sp_addsub
_sp_addsub:
    add x0, sp, x1
    add sp, x1, x2
    sub x3, sp, x4
    sub sp, x5, x6
    adds x7, sp, x8
    subs x9, sp, x10
    cmp sp, x11
    cmn sp, x12
    add x13, sp, x14, lsl #4
    sub sp, x15, x16, lsl #2
    add w0, wsp, w1
    add wsp, w1, w2
    sub w3, wsp, w4
    sub wsp, w5, w6
    adds w7, wsp, w8
    subs w9, wsp, w10
    cmp wsp, w11
    cmn wsp, w12
    add w13, wsp, w14, lsl #4
    sub wsp, w15, w16, lsl #2
    add w17, wsp, w18, uxtw #3
    sub wsp, w19, w20, sxtw #4
    sub sp, sp, x16
    add sp, sp, x16
    sub wsp, wsp, w16
    add wsp, wsp, w16
    ret
