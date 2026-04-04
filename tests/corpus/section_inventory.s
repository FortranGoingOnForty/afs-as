.section __TEXT,__cstring
msg:
    .asciz "hello"

.section __TEXT,__const
const_word:
    .quad 42

.section __DATA,__data
value:
    .byte 7

.section __DATA,__bss
    .p2align 4
scratch:
    .space 16
