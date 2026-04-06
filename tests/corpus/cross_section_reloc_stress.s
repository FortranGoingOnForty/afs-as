.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _cross_section_reloc_stress
.private_extern _local_hidden
.weak_reference _puts
.extern _ext
.extern _other
.p2align 2
_cross_section_reloc_stress:
    adrp x0, msg@PAGE
    add x0, x0, msg@PAGEOFF
    adrp x1, const_ptr@PAGE
    add x1, x1, const_ptr@PAGEOFF
    adrp x2, lit16@PAGE
    ldr q0, [x2, lit16@PAGEOFF]
    adrp x3, data_ptr@PAGE
    add x3, x3, data_ptr@PAGEOFF
    adrp x4, _scratch_buf@PAGE
    add x4, x4, _scratch_buf@PAGEOFF
    adrp x5, _tls_value$tlv$init@PAGE
    add x5, x5, _tls_value$tlv$init@PAGEOFF
    adrp x6, _tls_counter$tlv$init@PAGE
    add x6, x6, _tls_counter$tlv$init@PAGEOFF
    adrp x7, _ext@GOTPAGE
    ldr x7, [x7, _ext@GOTPAGEOFF]
    bl _puts
    bl _local_hidden
    ret
_local_hidden:
    ret

.section __TEXT,__cstring,cstring_literals
msg:
    .asciz "cross-section"
msg_tail:
    .asciz "reloc-stress"

.section __TEXT,__const
.p2align 3
const_ptr:
    .quad data_ptr
const_delta:
    .quad _other - _ext + 8
const_got:
    .quad _ext@GOT
const_msg:
    .quad msg
const_lit:
    .quad lit16
const_bss:
    .quad _scratch_buf
const_tls_data:
    .quad _tls_value$tlv$init
const_tls_bss:
    .quad _tls_counter$tlv$init

.section __TEXT,__literal16,16byte_literals
.p2align 4
lit16:
    .quad 0x0102030405060708
    .quad 0x1112131415161718
lit16_next:
    .quad 0x2122232425262728
    .quad 0x3132333435363738

.section __DATA,__data
.p2align 3
data_ptr:
    .quad msg_tail
data_const:
    .quad const_ptr
data_lit:
    .quad lit16_next
data_bss:
    .quad _scratch_buf
data_tls_data:
    .quad _tls_value$tlv$init
data_tls_bss:
    .quad _tls_counter$tlv$init
data_got:
    .quad _ext@GOT

.zerofill __DATA,__bss,_scratch_buf,16,4

.section __DATA,__thread_data,thread_local_regular
.p2align 3
_tls_value$tlv$init:
    .quad data_ptr

.tbss _tls_counter$tlv$init, 8, 3

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
