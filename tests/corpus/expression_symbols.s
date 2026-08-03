.set ABS1, 7

.build_version macos, 15, 0
.text
.globl _main
_main:
    movz x0, #ABS1
    ret

foo:
    nop
bar:
    ret

.data
    .quad ABS1
    .quad foo
    .quad bar - foo
    .quad _ext
    .quad _ext - _other + 4
