.build_version macos, 15, 0
.text
.globl _shifted_addsub
_shifted_addsub:
    add x0, x0, x1, lsl #3
    sub w2, w2, w3, asr #5
    cmp x4, x5, lsr #2
    cmn x6, x7, lsl #1
    neg x8, x9, lsl #2
    ret
