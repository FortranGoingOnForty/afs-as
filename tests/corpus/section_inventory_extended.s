.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _section_inventory_extended
.p2align 2
_section_inventory_extended:
    ret

.section __TEXT,__cstring,cstring_literals
msg:
    .asciz "hello"

.section __TEXT,__const
const_word:
    .quad 42

.section __TEXT,__literal16,16byte_literals
.p2align 4
lit16:
    .quad 1
    .quad 2

.section __DATA,__data
value:
    .byte 7

.section __DATA,__const
.p2align 3
const_data_word:
    .quad 99

.section __DATA,__thread_data,thread_local_regular
.p2align 2
_tls_value$tlv$init:
    .long 5

.section __DATA,__thread_vars,thread_local_variables
.globl _tls_value
_tls_value:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_value$tlv$init

.tbss _tls_counter$tlv$init, 4, 2

.section __DATA,__thread_vars,thread_local_variables
.globl _tls_counter
_tls_counter:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_counter$tlv$init

.section __DATA,__bss
    .p2align 4
scratch:
    .space 16

.subsections_via_symbols
