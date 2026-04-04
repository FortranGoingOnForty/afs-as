.section __TEXT,__text,regular,pure_instructions
.build_version macos, 15, 0 sdk_version 15, 5
.globl _call_puts
.p2align 2
_call_puts:
.cfi_startproc
Lloh0:
    adrp x0, l_.str@PAGE
Lloh1:
    add x0, x0, l_.str@PAGEOFF
    b _puts
    .loh AdrpAdd Lloh0, Lloh1
.cfi_endproc

.section __TEXT,__cstring,cstring_literals
l_.str:
    .asciz "hi"

.subsections_via_symbols
