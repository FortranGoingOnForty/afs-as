.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _helper_step
.p2align 2
_helper_step:
    mov x0, #11
    ret

.data
.globl _shared_value
.p2align 3
_shared_value:
    .quad 29

.subsections_via_symbols
