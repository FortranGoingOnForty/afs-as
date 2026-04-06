.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _main
.extern _helper_step
.extern _shared_value
.private_extern _hidden_step
.weak_definition _local_optional
.weak_reference _puts
.comm _common_counter, 8, 3
.p2align 2
_main:
    adrp x0, msg@PAGE
    add x0, x0, msg@PAGEOFF
    bl _puts

    mov x19, #0

    bl _hidden_step
    add x19, x19, x0

    bl _local_optional
    add x19, x19, x0

    bl _helper_step
    add x19, x19, x0

    adrp x8, _shared_value@GOTPAGE
    ldr x8, [x8, _shared_value@GOTPAGEOFF]
    ldr x0, [x8]
    add x19, x19, x0

    adrp x9, _common_counter@PAGE
    add x9, x9, _common_counter@PAGEOFF
    mov x10, #9
    str x10, [x9]
    ldr x0, [x9]
    add x19, x19, x0

    adrp x11, _scratch_buf@PAGE
    add x11, x11, _scratch_buf@PAGEOFF
    mov x12, #1
    str x12, [x11]
    ldr x0, [x11]
    add x19, x19, x0

    adrp x0, _tls_value@TLVPPAGE
    ldr x0, [x0, _tls_value@TLVPPAGEOFF]
    ldr x8, [x0]
    blr x8
    ldr x0, [x0]
    add x19, x19, x0

    adrp x0, _tls_counter@TLVPPAGE
    ldr x0, [x0, _tls_counter@TLVPPAGEOFF]
    ldr x8, [x0]
    blr x8
    mov x15, #7
    str x15, [x0]
    ldr x0, [x0]
    add x19, x19, x0

    mov x0, x19
    mov x16, #1
    svc #0x80

.p2align 2
_hidden_step:
    mov x0, #3
    ret

.p2align 2
_local_optional:
    mov x0, #2
    ret

.section __TEXT,__cstring,cstring_literals
msg:
    .asciz "mixed-link"

.zerofill __DATA,__bss,_scratch_buf,16,4

.section __DATA,__thread_data,thread_local_regular
_tls_value$tlv$init:
    .quad 5

.zerofill __DATA,__thread_bss,_tls_counter$tlv$init,8,3

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
