.text
.build_version macos, 11, 0 sdk_version 15, 5
.globl _directive_inventory
.set ABS_SET, 5
.equ ABS_EQU, ABS_SET + 2
.set ABS_ALL_ONES, 0xffffffffffffffff
.equ ABS_ALL_ONES_ALIAS, ABS_ALL_ONES
.set ABS_MIN, -9223372036854775808
.p2align 2
_directive_inventory:
    ret

.data
data_start:
    .word ABS_SET
    .long ABS_EQU
    .byte 0xaa
    .short 0x1234
    .quad _directive_inventory
    .quad ABS_ALL_ONES, ABS_ALL_ONES_ALIAS, ABS_MIN
    .ascii "hi"
    .string "bye"
    .skip 2
    .zero 3
    .fill 2, 2, 0x4142

.comm _common_buf, 8, 2
.zerofill __DATA,__bss,_scratch_buf,16,3

.subsections_via_symbols
