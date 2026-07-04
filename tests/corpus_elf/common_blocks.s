.text
.globl __prog_audit6_b2_common_block
.p2align 4
.type __prog_audit6_b2_common_block,@function
__prog_audit6_b2_common_block:
    pushq %rbp
    movq %rsp, %rbp
    subq $16, %rsp
    movq %rbx, -8(%rbp)
    movq %r12, -16(%rbp)
    leaq afs_common_myblock_0(%rip), %rax
    leaq afs_common_myblock_1(%rip), %rcx
    movl $10, %edx
    movl %edx, (%rax)
    movl $20, %ebx
    movl %ebx, (%rcx)
    movl $6, %r12d
    movl %r12d, %edi
    movl %edx, %esi
    xorl %eax, %eax
    call afs_write_int
    movl %r12d, %edi
    movl %ebx, %esi
    xorl %eax, %eax
    call afs_write_int
    movl %r12d, %edi
    xorl %eax, %eax
    call afs_write_newline
    movq -8(%rbp), %rbx
    movq -16(%rbp), %r12
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_audit6_b2_common_block, .-__prog_audit6_b2_common_block

.data
.comm afs_common_myblock_0,4,4
.comm afs_common_myblock_1,4,4

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_audit6_b2_common_block
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
