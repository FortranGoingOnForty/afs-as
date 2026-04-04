.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _main
.private_extern _helper
.weak_definition zlocal
.weak_reference _puts
.extern _ext
.set ABS1, 7
.comm _common,16,3
.p2align 2
_main:
bl _puts
ret
_helper:
ret
zlocal:
ret

.section __TEXT,__const
const_q:
.quad _helper - _ext + 8

.section __DATA,__data
data_q:
.quad _main
.long ABS1

.section __TEXT,__cstring,cstring_literals
msg:
.asciz "hi"

.zerofill __DATA,__bss,_scratch,8,3
.subsections_via_symbols
