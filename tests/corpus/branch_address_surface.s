.text
.globl _branch_address_surface
_branch_address_surface:
    adr x0, 1f
    tbz x1, #5, 2f
    add x2, x2, #1
2:
    tbnz x3, #33, 1f
    ret
    .p2align 2
1:
    nop
    adr x4, 1b
    ret
