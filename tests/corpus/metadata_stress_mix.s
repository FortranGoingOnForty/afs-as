.section __TEXT,__text,regular,pure_instructions
.build_version macos, 14, 1
.globl _entry
.private_extern _hidden
.weak_definition _weak_def
.weak_reference _puts
.p2align 2
_entry:
    bl _puts
    ret
_hidden:
    ret
_weak_def:
    ret

.comm _common_buf, 8, 2
.zerofill __DATA,__bss,_scratch_buf,16,3

.section __DATA,__thread_data,thread_local_regular
_tls_value$tlv$init:
    .quad 17

.zerofill __DATA,__thread_bss,_tls_counter$tlv$init,4,2

.section __DATA,__thread_vars,thread_local_variables
.globl _tls_value
_tls_value:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_value$tlv$init

.globl _tls_counter
_tls_counter:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_counter$tlv$init

.subsections_via_symbols
